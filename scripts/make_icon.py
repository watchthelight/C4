# <wtl> project: c4 | file: scripts/make_icon.py | author: watchthelight </wtl>
"""Write the C4 icon source: a 1024 by 1024 PNG with the uprooted.sh favicon geometry, a centered sage
square on near-black with three star pixels. The Tauri CLI turns it into the platform icon set:

    python scripts/make_icon.py assets/icons/c4-1024.png
    pnpm -C apps/desktop exec tauri icon ../../assets/icons/c4-1024.png -o src-tauri/icons

Standard library only, so the file is reproducible on any machine without an image library. The
output is deterministic: the same bytes every run.
"""

import struct
import sys
import zlib
from pathlib import Path

SIZE = 1024
BG = (8, 8, 12)  # --bg #08080c
SAGE = (106, 154, 110)  # --sage #6a9a6e
STAR = (200, 210, 220)  # --star rgb(200,210,220)
# (left, top, side) in pixels. The square is 448 wide and centered; the stars sit off the axes.
SQUARE = (288, 288, 448)
STARS = ((168, 176, 24), (800, 304, 16), (272, 824, 20))


def chunk(kind: bytes, data: bytes) -> bytes:
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)


def paint() -> bytearray:
    row_bytes = SIZE * 3
    canvas = bytearray(bytes(BG) * SIZE * SIZE)

    def fill(left: int, top: int, side: int, color: tuple) -> None:
        line = bytes(color) * side
        for y in range(top, top + side):
            start = y * row_bytes + left * 3
            canvas[start : start + side * 3] = line

    fill(*SQUARE, SAGE)
    for star in STARS:
        fill(*star, STAR)
    return canvas


def png_bytes() -> bytes:
    canvas = paint()
    row_bytes = SIZE * 3
    raw = bytearray()
    for y in range(SIZE):
        raw.append(0)  # filter type none
        raw += canvas[y * row_bytes : (y + 1) * row_bytes]
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def main(argv: list) -> int:
    if len(argv) != 2:
        print("usage: make_icon.py <output.png>", file=sys.stderr)
        return 2
    out = Path(argv[1])
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(png_bytes())
    print(f"wrote {out} ({out.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
