#!/bin/sh
# Rebuilds the PDF test files. Needs Ghostscript (ps2pdf) and Python with Pillow.
set -e
cd "$(dirname "$0")"
ps2pdf two-pages.ps two-pages.pdf
ps2pdf -sOwnerPassword=owner -sUserPassword=secret two-pages.ps locked.pdf
# A cut-off download: the first 60% of the file.
head -c "$(( $(wc -c < two-pages.pdf) * 6 / 10 ))" two-pages.pdf > truncated.pdf
# A scanned page: a picture and no text.
python3 - <<'PY'
from PIL import Image, ImageDraw
img = Image.new("RGB", (300, 424), "white")
d = ImageDraw.Draw(img)
for y in range(40, 380, 30):
    d.rectangle([30, y, 270, y + 12], fill=(40, 40, 40))
img.save("scanned.pdf", resolution=72)
PY
