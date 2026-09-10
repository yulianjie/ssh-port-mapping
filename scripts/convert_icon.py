"""Convert the editable SVG into PNG and a multi-resolution Windows ICO.

Dependencies: python -m pip install resvg-py Pillow
Run from any directory: python scripts/convert_icon.py
"""

from io import BytesIO
from pathlib import Path

from PIL import Image, ImageDraw
import resvg_py


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "assets"
SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)


def main():
    png = resvg_py.svg_to_bytes(
        svg_path=str(ASSETS / "portweave.svg"),
        width=1024,
        height=1024,
        skip_system_fonts=True,
    )
    master = Image.open(BytesIO(png)).convert("RGBA")
    master.save(ASSETS / "portweave.png")
    (ASSETS / "portweave-64.rgba").write_bytes(
        master.resize((64, 64), Image.Resampling.LANCZOS).tobytes()
    )
    master.save(
        ASSETS / "portweave.ico",
        sizes=[(size, size) for size in SIZES],
    )
    with Image.open(ASSETS / "portweave.ico") as icon:
        assert icon.ico.sizes() == {(size, size) for size in SIZES}
        for size in SIZES:
            frame = icon.ico.getimage((size, size)).convert("RGBA")
            assert frame.getpixel((0, 0))[3] <= 3  # Allow tiny Lanczos ringing.
            assert frame.getbbox() is not None

    preview = Image.new("RGB", (800, 440), "#eef2f6")
    draw = ImageDraw.Draw(preview)
    draw.rectangle((0, 0, 399, 439), fill="#0a1020")
    for origin in (0, 400):
        large = master.resize((256, 256), Image.Resampling.LANCZOS)
        preview.paste(large, (origin + 72, 30), large)
        draw.text((origin + 28, 312), "PortWeave / SSH tunnels", fill="#8799ae")
        x = origin + 28
        for size in (16, 24, 32, 48, 64):
            small = master.resize((size, size), Image.Resampling.LANCZOS)
            preview.paste(small, (x, 346 + (64 - size) // 2), small)
            draw.text((x, 417), str(size), fill="#8799ae")
            x += size + 22
    preview.save(ASSETS / "portweave-preview.png")
    print(f"Verified ICO sizes: {SIZES}; transparent corners present.")


if __name__ == "__main__":
    main()
