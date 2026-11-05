#!/usr/bin/env python3
"""Times poppler's command-line tools on a set of PDFs, for the finder's PDF preview.

See ../2026-11-05-pdf-preview-poppler.md.

Usage:
  2026-11-05-pdf_measure.py corpus DIR   build the test PDFs in DIR (needs soffice, ps2pdf, Pillow)
  2026-11-05-pdf_measure.py run DIR      time pdfinfo, pdftoppm and pdftotext on them
  2026-11-05-pdf_measure.py slow DIR     follow-up: other render settings on the slow vector page
  2026-11-05-pdf_measure.py zoom DIR     zoomed pages: whole page at ×1–×4 against a pane-sized slice
"""
import os
import random
import shutil
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

REAL = {
    "testpage": "/usr/share/cups/data/default-testpage.pdf",
    "spec": "/usr/share/doc/shared-mime-info/shared-mime-info-spec.pdf",
    "manual": "/usr/share/doc/printer-driver-foo2zjs/manual.pdf",
}
TIMEOUT_S = 30


def corpus(d: Path) -> None:
    d.mkdir(parents=True, exist_ok=True)
    for name, src in REAL.items():
        shutil.copy(src, d / f"{name}.pdf")

    # A long text document, made the way a user's would be: by an office suite, with embedded fonts.
    rng = random.Random(1)
    words = ("lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor "
             "incididunt ut labore et dolore magna aliqua").split()
    lines = [" ".join(rng.choice(words) for _ in range(12)) for _ in range(50_000)]
    (d / "longtext.txt").write_text("\n".join(lines))
    profile = d / "lo-profile"
    t = time.perf_counter()
    subprocess.run(["soffice", f"-env:UserInstallation=file://{profile}", "--headless",
                    "--convert-to", "pdf", "--outdir", str(d), str(d / "longtext.txt")],
                   check=True, capture_output=True)
    print(f"soffice longtext.txt -> pdf: {time.perf_counter() - t:.1f} s")

    # Scanned pages: 20 A4 pages at 300 dpi, JPEG, with grain so they compress like real scans.
    from PIL import Image, ImageDraw
    w, h = 2480, 3508
    pages = []
    for i in range(20):
        img = Image.new("L", (w, h), 235)
        draw = ImageDraw.Draw(img)
        prng = random.Random(i)
        y = 250
        while y < h - 250:
            x = 200
            while x < w - 200:
                ww = prng.randint(40, 220)
                draw.rectangle([x, y, min(x + ww, w - 200), y + 34], fill=prng.randint(20, 70))
                x += ww + 30
            y += 70
        grain = Image.effect_noise((w, h), 30)
        img = Image.blend(img, grain, 0.12).convert("RGB")
        pages.append(img)
    pages[0].save(d / "scan20.pdf", save_all=True, append_images=pages[1:], resolution=300, quality=75)

    # A heavy vector page, like a map or a CAD drawing: 200,000 coloured line segments.
    vrng = random.Random(2)
    ps = ["%!PS", "<< /PageSize [595 842] >> setpagedevice", "0.3 setlinewidth",
          "/l { moveto lineto stroke } def"]
    for _ in range(200_000):
        ps.append(f"{vrng.random():.2f} {vrng.random():.2f} {vrng.random():.2f} setrgbcolor "
                  f"{vrng.uniform(0, 595):.1f} {vrng.uniform(0, 842):.1f} "
                  f"{vrng.uniform(0, 595):.1f} {vrng.uniform(0, 842):.1f} l")
    ps.append("showpage")
    (d / "vector.ps").write_text("\n".join(ps))
    subprocess.run(["ps2pdf", str(d / "vector.ps"), str(d / "vector.pdf")], check=True)

    # Broken files: a cut-off download, and junk with a PDF header.
    data = (d / "manual.pdf").read_bytes()
    (d / "truncated.pdf").write_bytes(data[: len(data) * 6 // 10])
    (d / "garbage.pdf").write_bytes(b"%PDF-1.7\n" + random.Random(3).randbytes(200_000))

    for p in sorted(d.glob("*.pdf")):
        print(f"{p.name}: {p.stat().st_size} bytes")


def run_once(argv: list[str]) -> tuple[float, int, int, int]:
    """Returns (seconds, exit code, max RSS in KiB, stdout bytes)."""
    t = time.perf_counter()
    p = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    timer = threading.Timer(TIMEOUT_S, p.kill)
    timer.start()
    out = p.stdout.read()
    _, status, ru = os.wait4(p.pid, 0)
    elapsed = time.perf_counter() - t
    timer.cancel()
    p.returncode = os.waitstatus_to_exitcode(status)
    p.stdout.close()
    return elapsed, p.returncode, ru.ru_maxrss, len(out)


def measure(label: str, argv: list[str]) -> None:
    first, *_ = run_once(argv)  # warm-up: file and libraries into the page cache
    run_once(argv)
    n = 5 if first > 1.0 else 15
    runs = [run_once(argv) for _ in range(n)]
    ms = sorted(r[0] * 1000 for r in runs)
    p90 = ms[int(0.9 * (n - 1))]
    rss = max(r[2] for r in runs) / 1024
    codes = sorted({r[1] for r in runs})
    print(f"| {label} | {statistics.median(ms):.1f} | {p90:.1f} | {ms[0]:.1f} | {rss:.0f} | "
          f"{runs[-1][3]} | {codes} | {n} |", flush=True)


def pages(pdf: Path) -> int:
    out = subprocess.run(["pdfinfo", str(pdf)], capture_output=True, text=True).stdout
    for line in out.splitlines():
        if line.startswith("Pages:"):
            return int(line.split()[1])
    return 0


def run(d: Path) -> None:
    print("| case | median ms | p90 ms | min ms | max RSS MiB | stdout bytes | exit | runs |")
    print("|---|---|---|---|---|---|---|---|")
    measure("baseline: /bin/true", ["/bin/true"])
    order = ["testpage", "spec", "manual", "longtext", "scan20", "vector", "truncated", "garbage"]
    for name in order:
        pdf = d / f"{name}.pdf"
        f = str(pdf)
        n = pages(pdf)
        tag = f"{name} ({n} p)"
        measure(f"{tag}: pdfinfo", ["pdfinfo", f])
        measure(f"{tag}: pdftoppm p1 @16px (overhead)",
                ["pdftoppm", "-f", "1", "-l", "1", "-scale-to", "16", f])
        measure(f"{tag}: pdftoppm p1 @800px", ["pdftoppm", "-f", "1", "-l", "1", "-scale-to", "800", f])
        measure(f"{tag}: pdftoppm p1 @1600px", ["pdftoppm", "-f", "1", "-l", "1", "-scale-to", "1600", f])
        measure(f"{tag}: pdftoppm -png p1 @1600px",
                ["pdftoppm", "-png", "-f", "1", "-l", "1", "-scale-to", "1600", f])
        if n > 2:
            for pg in (n // 2, n):
                measure(f"{tag}: pdftoppm p{pg} @1600px",
                        ["pdftoppm", "-f", str(pg), "-l", str(pg), "-scale-to", "1600", f])
        measure(f"{tag}: pdftotext p1", ["pdftotext", "-layout", "-f", "1", "-l", "1", f, "-"])
        if n > 2:
            measure(f"{tag}: pdftotext all", ["pdftotext", "-layout", f, "-"])


def slow(d: Path) -> None:
    """Follow-up on the slow vector page: other settings and the cairo renderer, with a normal page to compare."""
    print("| case | median ms | p90 ms | min ms | max RSS MiB | stdout bytes | exit | runs |")
    print("|---|---|---|---|---|---|---|---|")
    for name in ("vector", "spec"):
        f = str(d / f"{name}.pdf")
        measure(f"{name}: pdftoppm -aaVector no p1 @1600px",
                ["pdftoppm", "-aaVector", "no", "-f", "1", "-l", "1", "-scale-to", "1600", f])
        # TIFF can't go to a pipe (only its 8-byte header arrives), so cairo writes to a file.
        out = str(d / "cairo-out")
        measure(f"{name}: pdftocairo -tiff (uncompressed) p1 @1600px",
                ["pdftocairo", "-tiff", "-tiffcompression", "none", "-singlefile", "-f", "1", "-l", "1",
                 "-scale-to", "1600", f, out])
        print(f"|   cairo-out.tif: {Path(out + '.tif').stat().st_size} bytes |")
        measure(f"{name}: pdftocairo -tiff -antialias none p1 @1600px",
                ["pdftocairo", "-tiff", "-tiffcompression", "none", "-antialias", "none", "-singlefile",
                 "-f", "1", "-l", "1", "-scale-to", "1600", f, out])
        print(f"|   cairo-out.tif: {Path(out + '.tif').stat().st_size} bytes |")


def zoom(d: Path) -> None:
    """Zoomed pages: whole page at fit-to-width ×1–×4, against only a pane-sized slice of a ×4 page.

    Assumes a preview pane 1000×900 pixels (for example 100×45 cells of 10×20 pixels)."""
    print("| case | median ms | p90 ms | min ms | max RSS MiB | stdout bytes | exit | runs |")
    print("|---|---|---|---|---|---|---|---|")
    pane_w, pane_h = 1000, 900
    for name in ("testpage", "spec", "manual", "longtext", "scan20"):
        f = str(d / f"{name}.pdf")
        for z in (1, 2, 3, 4):
            w = pane_w * z
            measure(f"{name}: whole page, width {w} (×{z})",
                    ["pdftoppm", "-f", "1", "-l", "1", "-scale-to-x", str(w), "-scale-to-y", "-1", f])
        w = pane_w * 4
        measure(f"{name}: {pane_w}×{pane_h} slice of width {w} (×4), from the middle",
                ["pdftoppm", "-f", "1", "-l", "1", "-scale-to-x", str(w), "-scale-to-y", "-1",
                 "-x", str(w // 2 - pane_w // 2), "-y", str(w // 2), "-W", str(pane_w), "-H", str(pane_h), f])


if __name__ == "__main__":
    cmds = {"corpus": corpus, "run": run, "slow": slow, "zoom": zoom}
    if len(sys.argv) != 3 or sys.argv[1] not in cmds:
        sys.exit(__doc__)
    cmds[sys.argv[1]](Path(sys.argv[2]))
