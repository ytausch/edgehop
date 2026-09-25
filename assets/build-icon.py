"""Render icon.svg and icon-small.svg into edgehop.ico.

Needs Inkscape on the PATH. Run from the repository root:

    python3 assets/build-icon.py
"""

import struct
import subprocess
import tempfile
from pathlib import Path

ASSETS = Path(__file__).parent
# The cursor turns to mush below 32 px, so the small sizes show only the ring.
SIZES = {16: "icon-small.svg", 20: "icon-small.svg", 24: "icon-small.svg"} | {
    size: "icon.svg" for size in (32, 40, 48, 64, 256)
}


def render(svg: Path, size: int, out: Path) -> bytes:
    subprocess.run(
        ["inkscape", svg, "-w", str(size), "-h", str(size), "-o", out],
        check=True,
        capture_output=True,
    )
    return out.read_bytes()


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        images = {
            size: render(ASSETS / svg, size, Path(tmp) / f"{size}.png")
            for size, svg in SIZES.items()
        }

    # ICONDIR, one ICONDIRENTRY per image, then the images as PNG.
    header = struct.pack("<HHH", 0, 1, len(images))
    entries = b""
    offset = len(header) + 16 * len(images)
    for size, png in images.items():
        # A width and height of 0 mean 256.
        entries += struct.pack(
            "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(png), offset
        )
        offset += len(png)
    (ASSETS / "edgehop.ico").write_bytes(header + entries + b"".join(images.values()))


if __name__ == "__main__":
    main()
