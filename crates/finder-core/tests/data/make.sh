#!/bin/sh
# Rebuilds the test files. Needs Ghostscript (ps2pdf), Python with Pillow, ffmpeg (with libx264) and LibreOffice.
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
# Videos: 2 seconds of ffmpeg's test pattern, 64×48, H.264 with a keyframe every 10 frames.
ffmpeg -v error -y -f lavfi -i testsrc=size=64x48:rate=25:duration=2 -c:v libx264 -pix_fmt yuv420p -g 10 \
    two-seconds.mp4
# The same, recorded turned 90° as phones do: a display matrix, with the pictures untouched.
ffmpeg -v error -y -display_rotation 90 -i two-seconds.mp4 -c copy rotated.mp4
# A cut-off download: the index (moov) is at the end, so it's lost.
head -c "$(( $(wc -c < two-seconds.mp4) * 6 / 10 ))" two-seconds.mp4 > truncated.mp4
# Office files (sheet.*, office.docx, deck.pptx, locked.*), written by LibreOffice through its Python bridge, so
# they're what a real program saves. Needs LibreOffice and python3-uno; Ubuntu's /usr/bin/python3 has it.
/usr/bin/python3 make_office.py
