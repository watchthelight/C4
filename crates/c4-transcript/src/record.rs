// <wtl> project: c4 | file: crates/c4-transcript/src/record.rs | author: watchthelight </wtl>
//! One transcript line. Claude Code writes several record types into one file: content records
//! (`user`, `assistant`, `system`, `attachment`) with a `uuid` and a `timestamp`, and metadata
//! records (`permission-mode`, `ai-title`, `cost-state`, `last-prompt`, ...) without either, which it
//! re-appends every turn so that the last one wins. The parser accepts all of them with every field
//! optional, because the format is Claude Code's own and changes without notice.

use c4_core::Timestamp;
use serde::Deserialize;
use serde::de::{Error as _, IgnoredAny};
use serde_json::Value;
use std::collections::BTreeMap;

/// The major version of Claude Code whose files the parser was written against.
pub const SUPPORTED_MAJOR: u32 = 2;

/// True when a record's `version` has the major version the parser knows. Other versions still
/// parse; the caller reports the mismatch once per file.
#[must_use]
pub fn version_supported(version: &str) -> bool {
    version
        .trim()
        .split('.')
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major == SUPPORTED_MAJOR)
}

/// Token usage of one API response.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
pub struct Usage {
    /// Fresh input tokens.
    #[serde(default)]
    pub input_tokens: u64,
    /// Tokens written to the prompt cache.
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    /// Tokens read from the prompt cache.
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    /// Output tokens.
    #[serde(default)]
    pub output_tokens: u64,
    /// The service tier, when reported.
    #[serde(default)]
    pub service_tier: Option<String>,
}

impl Usage {
    /// Everything the model read: fresh, cache writes and cache reads.
    #[must_use]
    pub fn total_input(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.cache_creation_input_tokens)
            .saturating_add(self.cache_read_input_tokens)
    }

    /// Field-wise maximum, the rule the status line uses when several records share one
    /// `message.id` (each carries the running total for that response).
    #[must_use]
    pub fn max_with(&self, other: &Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens.max(other.input_tokens),
            cache_creation_input_tokens: self
                .cache_creation_input_tokens
                .max(other.cache_creation_input_tokens),
            cache_read_input_tokens: self
                .cache_read_input_tokens
                .max(other.cache_read_input_tokens),
            output_tokens: self.output_tokens.max(other.output_tokens),
            service_tier: other
                .service_tier
                .clone()
                .or_else(|| self.service_tier.clone()),
        }
    }
}

/// One content block of a message.
#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
pub struct Block {
    /// `text`, `thinking`, `tool_use`, `tool_result`, `image`.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// The block id (`tool_use`).
    #[serde(default)]
    pub id: Option<String>,
    /// The tool name (`tool_use`).
    #[serde(default)]
    pub name: Option<String>,
    /// The tool input (`tool_use`), kept as JSON.
    #[serde(default)]
    pub input: Option<Value>,
    /// The text (`text`).
    #[serde(default)]
    pub text: Option<String>,
    /// The tool use answered (`tool_result`).
    #[serde(default)]
    pub tool_use_id: Option<String>,
    /// Whether the tool failed (`tool_result`).
    #[serde(default)]
    pub is_error: Option<bool>,
}

/// A message body: a plain string for typed prompts, blocks otherwise.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(untagged)]
pub enum Content {
    /// A plain string.
    Text(String),
    /// Content blocks.
    Blocks(Vec<Block>),
    /// Nothing, or a shape this version does not know.
    Other(IgnoredAny),
}

impl Default for Content {
    fn default() -> Self {
        Content::Other(IgnoredAny)
    }
}

impl Content {
    /// The blocks, or none for a string.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        match self {
            Content::Blocks(b) => b,
            _ => &[],
        }
    }

    /// The text of a string body or of every `text` block, joined with newlines.
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Content::Text(s) => s.clone(),
            Content::Blocks(blocks) => blocks
                .iter()
                .filter(|b| b.kind == "text")
                .filter_map(|b| b.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n"),
            Content::Other(_) => String::new(),
        }
    }
}

/// The `message` object of a content record.
#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
pub struct Message {
    /// The API message id; several assistant records can share one.
    #[serde(default)]
    pub id: Option<String>,
    /// The model, or `<synthetic>` for messages Claude Code made up.
    #[serde(default)]
    pub model: Option<String>,
    /// `user` or `assistant`.
    #[serde(default)]
    pub role: Option<String>,
    /// The body.
    #[serde(default)]
    pub content: Content,
    /// `tool_use`, `end_turn`, `stop_sequence`, `refusal`, or null mid-response.
    #[serde(default)]
    pub stop_reason: Option<String>,
    /// Token usage, on assistant records.
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// `compactMetadata` of a `compact_boundary` system record.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactMetadata {
    /// `auto` or `manual`.
    #[serde(default)]
    pub trigger: Option<String>,
    /// Context size before.
    #[serde(default)]
    pub pre_tokens: Option<u64>,
    /// Context size after.
    #[serde(default)]
    pub post_tokens: Option<u64>,
    /// Tokens dropped over the session.
    #[serde(default)]
    pub cumulative_dropped_tokens: Option<u64>,
    /// How long the compaction took.
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

/// Per-model totals of a `cost-state` record.
#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    /// Fresh input tokens.
    #[serde(default)]
    pub input_tokens: u64,
    /// Output tokens.
    #[serde(default)]
    pub output_tokens: u64,
    /// Thinking tokens.
    #[serde(default)]
    pub thinking_tokens: u64,
    /// Cache reads.
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    /// Cache writes.
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    /// Cost in dollars.
    #[serde(default, rename = "costUSD")]
    pub cost_usd: f64,
}

/// One line of a transcript. The `type` field says which other fields matter; everything is
/// optional.
#[derive(Clone, PartialEq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// `user`, `assistant`, `system`, `attachment`, or a metadata type such as `cost-state`.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// The system subtype (`turn_duration`, `compact_boundary`, `stop_hook_summary`, ...).
    #[serde(default)]
    pub subtype: Option<String>,
    /// The record id.
    #[serde(default)]
    pub uuid: Option<String>,
    /// The record this one follows.
    #[serde(default)]
    pub parent_uuid: Option<String>,
    /// When it was written, RFC 3339.
    #[serde(default)]
    pub timestamp: Option<String>,
    /// The session id (equals the file stem).
    #[serde(default)]
    pub session_id: Option<String>,
    /// The working directory at the time.
    #[serde(default)]
    pub cwd: Option<String>,
    /// The git branch at the time.
    #[serde(default)]
    pub git_branch: Option<String>,
    /// The Claude Code version.
    #[serde(default)]
    pub version: Option<String>,
    /// True in agent files.
    #[serde(default)]
    pub is_sidechain: bool,
    /// The agent id, in agent files.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// `bg` for background sessions.
    #[serde(default)]
    pub session_kind: Option<String>,
    /// The plan slug.
    #[serde(default)]
    pub slug: Option<String>,
    /// System-injected user records.
    #[serde(default)]
    pub is_meta: bool,
    /// The summary record after a compaction.
    #[serde(default)]
    pub is_compact_summary: bool,
    /// An assistant record that stands for an API error.
    #[serde(default)]
    pub is_api_error_message: bool,
    /// `typed`, `queued`, `system`, `sdk`.
    #[serde(default)]
    pub prompt_source: Option<String>,
    /// The message, on content records.
    #[serde(default)]
    pub message: Option<Message>,
    /// The structured tool result, whose shape depends on the tool.
    #[serde(default)]
    pub tool_use_result: Option<Value>,
    /// `turn_duration`: the turn length.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// `turn_duration`: records in the turn.
    #[serde(default)]
    pub message_count: Option<u32>,
    /// `turn_duration`: background agents still running.
    #[serde(default)]
    pub pending_background_agent_count: Option<u32>,
    /// `turn_duration`: workflows still running.
    #[serde(default)]
    pub pending_workflow_count: Option<u32>,
    /// `compact_boundary`: what the compaction did.
    #[serde(default)]
    pub compact_metadata: Option<CompactMetadata>,
    /// `permission-mode`: the mode.
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// `ai-title`: the title Claude Code gave the session.
    #[serde(default)]
    pub ai_title: Option<String>,
    /// `agent-name`: the name given with `--name` or `/rename`.
    #[serde(default)]
    pub agent_name: Option<String>,
    /// `cost-state`: total cost.
    #[serde(default, rename = "totalCostUSD")]
    pub total_cost_usd: Option<f64>,
    /// `cost-state`: per-model totals.
    #[serde(default)]
    pub model_usage: Option<BTreeMap<String, ModelUsage>>,
    /// `continued-in`: the session that continued this one.
    #[serde(default)]
    pub continued_in_session_id: Option<String>,
}

impl Record {
    /// Parse one line. Fails when the line is not a JSON object.
    pub fn parse(line: &str) -> Result<Record, serde_json::Error> {
        if !line.trim_start().starts_with('{') {
            return Err(serde_json::Error::custom("not a JSON object"));
        }
        serde_json::from_str(line)
    }

    /// The timestamp as a [`Timestamp`], when the record has one that parses.
    #[must_use]
    pub fn at(&self) -> Option<Timestamp> {
        self.timestamp.as_deref().and_then(Timestamp::parse_rfc3339)
    }

    /// True for the records that carry a `uuid` and a `timestamp`.
    #[must_use]
    pub fn is_content(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "user" | "assistant" | "system" | "attachment"
        )
    }

    /// The `tool_use` blocks of an assistant record.
    pub fn tool_uses(&self) -> impl Iterator<Item = &Block> {
        self.message
            .iter()
            .flat_map(|m| m.content.blocks().iter())
            .filter(|b| b.kind == "tool_use")
    }

    /// The `tool_result` blocks of a user record.
    pub fn tool_results(&self) -> impl Iterator<Item = &Block> {
        self.message
            .iter()
            .flat_map(|m| m.content.blocks().iter())
            .filter(|b| b.kind == "tool_result")
    }

    /// True for a user record that is a prompt the user (or the SDK) sent, not a tool result and
    /// not something Claude Code injected.
    #[must_use]
    pub fn is_user_prompt(&self) -> bool {
        if self.kind != "user" || self.is_meta || self.is_compact_summary {
            return false;
        }
        match self.message.as_ref().map(|m| &m.content) {
            Some(Content::Text(s)) => !s.trim().is_empty(),
            Some(Content::Blocks(blocks)) => {
                blocks.iter().any(|b| b.kind == "text" || b.kind == "image")
                    && !blocks.iter().any(|b| b.kind == "tool_result")
            }
            _ => false,
        }
    }

    /// The agent an `Agent` tool result names: `(agent_id, is_async)`. A background launch has
    /// `status: async_launched`; a foreground agent's result arrives when it has returned.
    #[must_use]
    pub fn launched_agent(&self) -> Option<(String, bool)> {
        let result = self.tool_use_result.as_ref()?.as_object()?;
        let agent_id = result.get("agentId")?.as_str()?.to_string();
        let status = result.get("status").and_then(Value::as_str);
        let is_async = result
            .get("isAsync")
            .and_then(Value::as_bool)
            .unwrap_or(status == Some("async_launched"));
        Some((agent_id, is_async))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_versions() {
        assert!(version_supported("2.1.299"));
        assert!(version_supported(" 2.0.0 "));
        assert!(!version_supported("3.0.0"));
        assert!(!version_supported("1.0.120"));
        assert!(!version_supported("dev"));
    }

    #[test]
    fn parses_a_prompt_and_an_assistant_record() {
        let user = Record::parse(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-10-25T12:00:00.000Z","sessionId":"s","cwd":"/x","version":"2.1.299","message":{"role":"user","content":"hello"},"promptSource":"typed","unknownField":{"a":1}}"#,
        )
        .unwrap();
        assert!(user.is_user_prompt());
        assert_eq!(user.at(), Timestamp::parse_rfc3339("2026-10-25T12:00:00Z"));
        assert!(user.is_content());
        let assistant = Record::parse(
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-10-25T12:00:01.000Z","message":{"id":"msg_1","model":"m","role":"assistant","content":[{"type":"text","text":"ok"},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}],"stop_reason":"tool_use","usage":{"input_tokens":3,"cache_read_input_tokens":10,"output_tokens":7}}}"#,
        )
        .unwrap();
        assert!(!assistant.is_user_prompt());
        let uses: Vec<&Block> = assistant.tool_uses().collect();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].name.as_deref(), Some("Bash"));
        assert_eq!(uses[0].id.as_deref(), Some("toolu_1"));
        let usage = assistant.message.as_ref().unwrap().usage.clone().unwrap();
        assert_eq!(usage.total_input(), 13);
        assert_eq!(assistant.message.as_ref().unwrap().content.text(), "ok");
    }

    #[test]
    fn tool_results_and_launches() {
        let r = Record::parse(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_2","content":"x","is_error":true}]},"toolUseResult":{"agentId":"ag1","status":"async_launched","isAsync":true}}"#,
        )
        .unwrap();
        assert!(!r.is_user_prompt());
        let results: Vec<&Block> = r.tool_results().collect();
        assert_eq!(results[0].is_error, Some(true));
        assert_eq!(r.launched_agent(), Some(("ag1".into(), true)));
        let sync = Record::parse(
            r#"{"type":"user","toolUseResult":{"agentId":"ag2","status":"completed"}}"#,
        )
        .unwrap();
        assert_eq!(sync.launched_agent(), Some(("ag2".into(), false)));
        let bash =
            Record::parse(r#"{"type":"user","toolUseResult":{"stdout":"","stderr":""}}"#).unwrap();
        assert_eq!(bash.launched_agent(), None);
    }

    #[test]
    fn metadata_records_and_odd_shapes() {
        let title = Record::parse(r#"{"type":"ai-title","aiTitle":"Alpha"}"#).unwrap();
        assert_eq!(title.ai_title.as_deref(), Some("Alpha"));
        assert!(!title.is_content());
        assert_eq!(title.at(), None);
        let cost = Record::parse(
            r#"{"type":"cost-state","totalCostUSD":0.42,"modelUsage":{"m":{"inputTokens":1,"outputTokens":2,"costUSD":0.42}}}"#,
        )
        .unwrap();
        assert!(
            cost.total_cost_usd
                .is_some_and(|c| (c - 0.42).abs() < f64::EPSILON)
        );
        let per_model = cost.model_usage.unwrap();
        assert_eq!(per_model["m"].output_tokens, 2);
        assert!((per_model["m"].cost_usd - 0.42).abs() < f64::EPSILON);
        let system = Record::parse(
            r#"{"type":"system","subtype":"turn_duration","durationMs":10,"pendingBackgroundAgentCount":1,"pendingWorkflowCount":0}"#,
        )
        .unwrap();
        assert_eq!(system.pending_background_agent_count, Some(1));
        let odd =
            Record::parse(r#"{"type":"user","message":{"role":"user","content":42}}"#).unwrap();
        assert_eq!(odd.message.unwrap().content, Content::Other(IgnoredAny));
        assert!(Record::parse("[1,2]").is_err());
        assert!(Record::parse("").is_err());
        assert!(Record::parse("{\"type\":").is_err());
        let meta = Record::parse(
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"injected"}}"#,
        )
        .unwrap();
        assert!(!meta.is_user_prompt());
    }

    #[test]
    fn usage_max_merge() {
        let a = Usage {
            input_tokens: 12,
            output_tokens: 9,
            ..Usage::default()
        };
        let b = Usage {
            input_tokens: 12,
            output_tokens: 38,
            cache_creation_input_tokens: 5,
            ..Usage::default()
        };
        let m = a.max_with(&b);
        assert_eq!(m.output_tokens, 38);
        assert_eq!(m.cache_creation_input_tokens, 5);
        assert_eq!(m.input_tokens, 12);
    }
}
