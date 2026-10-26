// <wtl> project: c4 | file: crates/c4-ingest/src/acl.rs | author: watchthelight </wtl>
//! The security attributes of the ingest pipe: a DACL that admits the current user and nobody
//! else, built from the SID of this process's token through the SDDL string `D:P(A;;GA;;;<sid>)`
//! (protected DACL, one access-allowed entry granting generic all to that SID).
//!
//! This is the one place in the crate that calls Win32 directly, so it is the one place that
//! allows `unsafe`. Each call sits in a few lines with its invariants written next to it.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::core::PWSTR;

/// A `SECURITY_ATTRIBUTES` whose descriptor this struct owns.
pub struct SecurityAttributes {
    attrs: SECURITY_ATTRIBUTES,
    sddl: String,
}

// SAFETY: the descriptor is heap memory from `LocalAlloc` that only `Drop` frees; nothing in the
// struct is tied to the thread that built it.
unsafe impl Send for SecurityAttributes {}

impl SecurityAttributes {
    /// A DACL for the user this process runs as.
    pub fn current_user_only() -> io::Result<Self> {
        let sid = current_user_sid()?;
        let sddl = format!("D:P(A;;GA;;;{sid})");
        let descriptor = descriptor_from_sddl(&sddl)?;
        Ok(Self {
            attrs: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
            sddl,
        })
    }

    /// The SDDL the descriptor was built from.
    #[must_use]
    pub fn sddl(&self) -> &str {
        &self.sddl
    }

    /// The pointer `CreateNamedPipeW` takes. Valid for as long as `self` lives.
    pub fn as_ptr(&mut self) -> *mut c_void {
        ptr::from_mut(&mut self.attrs).cast()
    }
}

impl Drop for SecurityAttributes {
    fn drop(&mut self) {
        // SAFETY: the descriptor came from ConvertStringSecurityDescriptorToSecurityDescriptorW,
        // which documents LocalFree as its release, and this is the only place that frees it.
        unsafe {
            LocalFree(self.attrs.lpSecurityDescriptor);
        }
    }
}

/// The string SID (`S-1-5-...`) of the user this process runs as.
pub fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing; OpenProcessToken
    // writes a real handle into `token` on success and reports failure through its return value.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid = token_user_sid(token);
    // SAFETY: the handle came from OpenProcessToken above and is closed exactly once.
    unsafe {
        CloseHandle(token);
    }
    sid
}

fn token_user_sid(token: HANDLE) -> io::Result<String> {
    let mut len: u32 = 0;
    // SAFETY: a null buffer of length 0 asks for the size; the call writes only `len`.
    let ok = unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &raw mut len) };
    if ok == 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            return Err(e);
        }
    }
    if len == 0 {
        return Err(io::Error::other("GetTokenInformation reported no size"));
    }
    // u64 slots keep the buffer aligned for TOKEN_USER, whose alignment is that of a pointer.
    let mut buf: Vec<u64> = vec![0; (len as usize).div_ceil(size_of::<u64>())];
    // SAFETY: `buf` holds at least `len` bytes, aligned for TOKEN_USER; the call writes at most
    // `len` bytes into it.
    let ok = unsafe {
        GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &raw mut len)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: on success the buffer starts with a TOKEN_USER whose Sid points inside the same
    // buffer, which stays alive until this function returns.
    let sid: PSID = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    sid_to_string(sid)
}

fn sid_to_string(sid: PSID) -> io::Result<String> {
    let mut text: PWSTR = ptr::null_mut();
    // SAFETY: ConvertSidToStringSidW allocates the string with LocalAlloc and writes its pointer
    // into `text`; `sid` points at a SID the caller keeps alive for the call.
    if unsafe { ConvertSidToStringSidW(sid, &raw mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `text` is a NUL-terminated UTF-16 string from the call above; it is read up to the
    // NUL and freed exactly once.
    let value = unsafe {
        let mut n = 0;
        while *text.add(n) != 0 {
            n += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
        LocalFree(text.cast());
        value
    };
    Ok(value)
}

fn descriptor_from_sddl(sddl: &str) -> io::Result<PSECURITY_DESCRIPTOR> {
    let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated and outlives the call; the descriptor is written into
    // `descriptor` and belongs to the caller, who frees it with LocalFree.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &raw mut descriptor,
            ptr::null_mut(),
        )
    };
    if ok == 0 || descriptor.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dacl_names_the_current_user() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-5-"), "{sid}");
        let mut attrs = SecurityAttributes::current_user_only().unwrap();
        assert_eq!(attrs.sddl(), format!("D:P(A;;GA;;;{sid})"));
        assert!(!attrs.as_ptr().is_null());
    }
}
