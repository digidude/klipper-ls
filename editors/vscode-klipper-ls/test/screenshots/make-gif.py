"""Assemble the README hero GIF from the frames the screenshot suite wrote.

    uv run --with pillow python make-gif.py <frames dir> <out.gif> [width]

<frames dir>/frames.json lists {file, ms} in order. Frames are captured at 2x
and scaled down, which keeps the text crisp at the width GitHub shows.
"""

import json
import sys
from pathlib import Path

from PIL import Image

frames_dir, out = Path(sys.argv[1]), Path(sys.argv[2])
width = int(sys.argv[3]) if len(sys.argv) > 3 else 1000

manifest = json.loads((frames_dir / "frames.json").read_text())
images, durations = [], []
for entry in manifest:
    image = Image.open(frames_dir / entry["file"]).convert("RGB")
    height = round(image.height * width / image.width)
    image = image.resize((width, height), Image.LANCZOS)
    images.append(image.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
    durations.append(entry["ms"])

images[0].save(out, save_all=True, append_images=images[1:], duration=durations, loop=0, optimize=True)
print(f"{out}: {len(images)} frames, {out.stat().st_size // 1024} KiB")
