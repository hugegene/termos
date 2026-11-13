#!/usr/bin/env python3
"""Makes the sample folder the finder demo walks through: one file of each kind, about a made-up bakery.

    python3 scripts/demo/make_files.py <folder> [--video FILE] [--image FILE]

Writes README.md, index.html, sales.csv, report.pdf, notes.docx and budget.xlsx, a video and a picture. The
video is mandelbrot.mp4 and the picture sunset.png, both made here, unless --video or --image gives one: then
it's copied as movie.<its extension> or snapshot.png. Those names keep the tour's order in the file list. --image can also be
a video, whose frame from 10% in is used, as the finder's preview shows it.

Needs Pillow, openpyxl and matplotlib (Python), LibreOffice (soffice) and ffmpeg. Everything else is drawn or
written here, so the demo shows no one's real files. Plan: docs/plans/2026-11-13-finder-demo-gif.md.
"""

import argparse

import csv
import math
import os
import random
import shutil
import subprocess
import tempfile
from pathlib import Path

MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]

README = """\
# Brightwater Bakery

Plans and numbers for the bakery's **winter season**.

## In this folder

| File | What it is |
|---|---|
| [index.html](index.html) | the shop's web page |
| [sales.csv](sales.csv) | loaves sold each month |
| [report.pdf](report.pdf) | the autumn report |
| [budget.xlsx](budget.xlsx) | the winter budget |

## To do

- [x] Bake the new rye loaf
- [ ] Order flour for December
- [ ] Print the holiday menu

> Opening hours change on *1 December*.
"""

# One element per line: the demo edits it in Neovim by moving down a fixed number of lines.
INDEX_HTML = """\
<!doctype html>
<html>
<head><title>Brightwater Bakery</title></head>
<body>
<h1>Brightwater Bakery</h1>
<p>Fresh bread every morning from <b>7 am</b>, on the corner of Harbour Street.</p>
<h2>Today's bakes</h2>
<ul>
<li>Sourdough loaf</li>
<li>Almond croissant</li>
<li>Cinnamon bun</li>
</ul>
<h2>Opening hours</h2>
<table>
<tr><th>Day</th><th>Hours</th></tr>
<tr><td>Monday to Friday</td><td>7:00 to 18:00</td></tr>
<tr><td>Weekends</td><td>8:00 to 14:00</td></tr>
</table>
<p>Questions? Ask at the counter.</p>
</body>
</html>
"""

NOTES_HTML = """\
<html><body>
<h1>Team meeting, 3 November</h1>
<p>Present: <b>Ana</b>, <b>Ravi</b>, <b>Mei</b> and <b>Tom</b>.</p>
<h2>Decisions</h2>
<ul>
<li>Open at <b>7 am</b> on weekdays from December.</li>
<li>Add a <i>rye loaf</i> to the bread shelf.</li>
<li>Try oat milk at no extra charge for a month.</li>
</ul>
<h2>Actions</h2>
<table border="1">
<tr><th>Who</th><th>What</th><th>By</th></tr>
<tr><td>Ana</td><td>Order flour for December</td><td>14 Nov</td></tr>
<tr><td>Ravi</td><td>Print the holiday menu</td><td>20 Nov</td></tr>
<tr><td>Mei</td><td>Update the web page</td><td>21 Nov</td></tr>
</table>
<h2>Next meeting</h2>
<p>Monday 17 November, before opening.</p>
</body></html>
"""


def sales_rows():
    rng = random.Random(7)
    rows = []
    for i, month in enumerate(MONTHS):
        season = 1 + 0.35 * math.cos((i - 11) / 12 * 2 * math.pi)
        sourdough = round(380 * season + rng.randint(-25, 25))
        croissant = round(520 * season + rng.randint(-30, 30))
        bun = round(300 * season + rng.randint(-20, 20))
        rows.append([month, sourdough, croissant, bun])
    return rows


def write_csv(path, rows):
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["month", "sourdough", "croissant", "cinnamon bun"])
        w.writerows(rows)


def chart(path, rows):
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots(figsize=(7, 3.4), dpi=110)
    x = range(len(rows))
    names = ["sourdough", "croissant", "cinnamon bun"]
    colours = ["#c8553d", "#f28f3b", "#588b8b"]
    bottom = [0] * len(rows)
    for k, (name, colour) in enumerate(zip(names, colours)):
        values = [r[k + 1] for r in rows]
        ax.bar(x, values, bottom=bottom, label=name, color=colour, width=0.7)
        bottom = [b + v for b, v in zip(bottom, values)]
    ax.set_xticks(list(x), [r[0] for r in rows])
    ax.set_ylabel("loaves and pastries")
    ax.set_title("Sold each month")
    ax.spines[["top", "right"]].set_visible(False)
    ax.legend(frameon=False, ncols=3, loc="upper center")
    ax.set_ylim(0, max(bottom) * 1.25)
    fig.tight_layout()
    fig.savefig(path)
    plt.close(fig)


def report_html(rows):
    total = [sum(r[k] for r in rows) for k in (1, 2, 3)]
    table = "\n".join(
        f"<tr><td>{r[0]}</td><td>{r[1]}</td><td>{r[2]}</td><td>{r[3]}</td></tr>" for r in rows
    )
    return f"""\
<html><body style="font-family: 'DejaVu Sans'">
<h1>Autumn report</h1>
<p><b>Brightwater Bakery</b>, October 2026</p>
<p>Sales rose through the autumn, as they do every year, and croissants sold best. The new rye loaf
sold out on most days of its first week, so it stays on the shelf.</p>
<p><img src="chart.png" width="620"></p>
<p>Winter is the busiest season. We plan to open earlier on weekdays and to order flour a month ahead.</p>
<h2 style="page-break-before: always">Sales by month</h2>
<table border="1" cellpadding="4">
<tr><th>Month</th><th>Sourdough</th><th>Croissant</th><th>Cinnamon bun</th></tr>
{table}
<tr><td><b>Total</b></td><td><b>{total[0]}</b></td><td><b>{total[1]}</b></td><td><b>{total[2]}</b></td></tr>
</table>
<h2>Next steps</h2>
<ul><li>Hire one more baker for the early shift.</li><li>Print the holiday menu by 20 November.</li></ul>
</body></html>
"""


def sunset(path):
    from PIL import Image, ImageChops, ImageDraw, ImageFilter

    w, h = 960, 540
    img = Image.new("RGB", (w, h))
    px = img.load()
    top, horizon = (36, 22, 74), (255, 140, 66)
    for y in range(h):
        t = min(y / (h * 0.62), 1.0) ** 1.4
        c = tuple(round(a + (b - a) * t) for a, b in zip(top, horizon))
        for x in range(w):
            px[x, y] = c
    # A soft glow around the sun, added on top of the sky.
    glow = Image.new("RGB", (w, h), (0, 0, 0))
    ImageDraw.Draw(glow).ellipse((480 - 150, 330 - 150, 480 + 150, 330 + 150), fill=(120, 60, 10))
    img = ImageChops.add(img, glow.filter(ImageFilter.GaussianBlur(70)))
    d = ImageDraw.Draw(img)
    d.ellipse((480 - 75, 330 - 75, 480 + 75, 330 + 75), fill=(255, 209, 102))
    rng = random.Random(3)
    layers = [((92, 42, 86), 300, 70), ((62, 28, 68), 360, 55), ((34, 16, 44), 420, 45)]
    for colour, base, amp in layers:
        pts = [(0, h)]
        p1, p2 = rng.uniform(0, 6), rng.uniform(0, 6)
        for x in range(0, w + 30, 30):
            y = base + amp * (0.6 * math.sin(x / 110 + p1) + 0.4 * math.sin(x / 47 + p2)) + rng.randint(-8, 8)
            pts.append((x, round(y)))
        pts.append((w, h))
        d.polygon(pts, fill=colour)
    img.save(path)


def soffice(src, fmt, out_dir, infilter=None):
    """Converts with LibreOffice, using its own profile so a running LibreOffice doesn't get in the way."""
    with tempfile.TemporaryDirectory() as profile:
        cmd = ["soffice", f"-env:UserInstallation=file://{profile}", "--headless"]
        if infilter:
            cmd.append(f"--infilter={infilter}")
        cmd += ["--convert-to", fmt, "--outdir", str(out_dir), str(src)]
        subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def budget(path, work):
    from datetime import date

    from openpyxl import Workbook
    from openpyxl.styles import Font

    wb = Workbook()
    ws = wb.active
    ws.title = "Winter"
    ws.append(["Item", "Supplier", "Due", "Cost"])
    items = [
        ("Flour, 40 sacks", "Millbrook Farm", date(2026, 11, 20), 1240),
        ("Butter, 60 kg", "Valley Dairy", date(2026, 11, 22), 690),
        ("Oven service", "HeatRight", date(2026, 12, 1), 380),
        ("Holiday menus", "Print Corner", date(2026, 11, 20), 145),
        ("Extra baker, 8 weeks", "", date(2026, 12, 1), 6400),
        ("Oat milk trial", "Valley Dairy", date(2026, 12, 1), 210),
    ]
    for row in items:
        ws.append(list(row))
    n = len(items) + 1
    ws.append(["Total", "", None, f"=SUM(D2:D{n})"])
    for c in ws[1] + ws[n + 1]:
        c.font = Font(bold=True)
    for r in range(2, n + 1):
        ws.cell(r, 3).number_format = "yyyy-mm-dd"
    sup = wb.create_sheet("Suppliers")
    sup.append(["Supplier", "Contact", "Delivers"])
    sup.append(["Millbrook Farm", "orders@millbrook.example", "Tuesdays"])
    sup.append(["Valley Dairy", "hello@valleydairy.example", "Daily"])
    sup.append(["HeatRight", "service@heatright.example", "On call"])
    sup.append(["Print Corner", "shop@printcorner.example", "Next day"])
    # openpyxl writes formulas without their values; LibreOffice works them out and saves them.
    raw = work / "budget.xlsx"
    wb.save(raw)
    out = work / "calc"
    soffice(raw, "xlsx", out)
    shutil.move(out / "budget.xlsx", path)


VIDEO_SUFFIXES = {".mp4", ".mov", ".mkv", ".webm", ".avi", ".m4v"}


def still(src, dest):
    """A picture for the demo: `src` copied, or, for a video, its frame from 10% in."""
    if src.suffix.lower() not in VIDEO_SUFFIXES:
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(src), "-frames:v", "1", str(dest)], check=True)
        return
    length = float(subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0",
                                   str(src)], check=True, capture_output=True, text=True).stdout)
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", f"{length * 0.1:.2f}", "-i", str(src), "-frames:v", "1",
                    str(dest)], check=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("folder", type=Path)
    ap.add_argument("--video", type=Path, help="a video to show instead of the Mandelbrot one")
    ap.add_argument("--image", type=Path, help="a picture (or a video's frame) to show instead of the sunset")
    args = ap.parse_args()
    folder = args.folder
    folder.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        (folder / "README.md").write_text(README)
        (folder / "index.html").write_text(INDEX_HTML)
        rows = sales_rows()
        write_csv(folder / "sales.csv", rows)
        if args.image:
            still(args.image, folder / "snapshot.png")
        else:
            sunset(folder / "sunset.png")

        chart(work / "chart.png", rows)
        (work / "report.html").write_text(report_html(rows))
        soffice(work / "report.html", "pdf:writer_pdf_Export", folder, "HTML (StarWriter)")

        (work / "notes.html").write_text(NOTES_HTML)
        soffice(work / "notes.html", 'docx:MS Word 2007 XML', folder, "HTML (StarWriter)")

        budget(folder / "budget.xlsx", work)

        if args.video:
            shutil.copy(args.video, folder / f"movie{args.video.suffix.lower()}")
        else:
            subprocess.run(
                ["ffmpeg", "-v", "error", "-y", "-f", "lavfi",
                 "-i", "mandelbrot=size=640x360:rate=25:inner=black:outer=normalized_iteration_count",
                 "-vf", "hue=h=-90:s=0.8,curves=all='0/0 0.6/0.25 1/1'",
                 "-t", "8", "-pix_fmt", "yuv420p", str(folder / "mandelbrot.mp4")],
                check=True,
            )
    for f in sorted(os.listdir(folder)):
        print(f, (folder / f).stat().st_size)


if __name__ == "__main__":
    main()
