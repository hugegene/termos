#!/usr/bin/env python3
"""Checks whether a terminal draws kitty-graphics placements with a source rectangle (x, y, w, h), and whether
placing again with the same placement id (p=) moves the picture instead of adding a second one.

See ../2026-11-07-kitty-source-rect.md.

Usage:
  2026-11-07-kitty_source_rect.py draw OUTDIR    run inside the terminal under test: draws three steps, writing
                                                 OUTDIR/step-N when each is on screen (for a screenshot)
  2026-11-07-kitty_source_rect.py query OUTDIR   run inside the terminal under test: writes its XTVERSION reply to
                                                 OUTDIR/xtversion.txt
  2026-11-07-kitty_source_rect.py ghostty OUTDIR runs `query`, then `draw`, in new Ghostty windows on $DISPLAY,
                                                 screenshots each step with ImageMagick's `import`, and counts
                                                 colours (needs xdotool, Pillow)

The test picture is 400×400 pixels in four 200×200 quadrants: red (top left), green (top right), blue (bottom
left), yellow (bottom right). It is sent once (a=t), then only placed (a=p):
  step 1: source rectangle = the yellow quadrant, placement 1, 20×10 cells
  step 2: the same placement id, source rectangle = the red quadrant (should replace yellow, not add to it)
  step 3: the same placement id, source rectangle = the bottom half (blue + yellow), 40×10 cells
"""
import base64
import os
import subprocess
import sys
import time
from pathlib import Path

ID = 7
QUADS = {"red": (255, 0, 0), "green": (0, 255, 0), "blue": (0, 0, 255), "yellow": (255, 255, 0)}


def picture() -> bytes:
    rows = []
    for y in range(400):
        row = bytearray()
        for x in range(400):
            row += bytes(QUADS[("red", "green", "blue", "yellow")[(y >= 200) * 2 + (x >= 200)]])
        rows.append(bytes(row))
    return b"".join(rows)


def apc(control: str, payload: bytes = b"") -> bytes:
    return b"\x1b_G" + control.encode() + (b";" + payload if payload else b"") + b"\x1b\\"


def draw(out: Path) -> None:
    w = sys.stdout.buffer
    w.write(b"\x1b[2J\x1b[H\x1b[?25l")
    data = base64.standard_b64encode(picture())
    chunks = [data[i:i + 4096] for i in range(0, len(data), 4096)]
    for i, c in enumerate(chunks):
        more = int(i + 1 < len(chunks))
        ctl = f"a=t,f=24,s=400,v=400,i={ID},q=2,m={more}" if i == 0 else f"m={more}"
        w.write(apc(ctl, c))
    steps = [
        f"a=p,i={ID},p=1,x=200,y=200,w=200,h=200,c=20,r=10,C=1,q=2",
        f"a=p,i={ID},p=1,x=0,y=0,w=200,h=200,c=20,r=10,C=1,q=2",
        f"a=p,i={ID},p=1,x=0,y=200,w=400,h=200,c=40,r=10,C=1,q=2",
    ]
    for n, ctl in enumerate(steps, 1):
        w.write(b"\x1b[3;3H" + apc(ctl))
        w.flush()
        time.sleep(0.7)
        (out / f"step-{n}").touch()
        time.sleep(1.5)
    w.write(apc(f"a=d,d=I,i={ID},q=2") + b"\x1b[?25h")
    w.flush()


def query(out: Path) -> None:
    """Asks the terminal for its name and version (XTVERSION, CSI > 0 q), with device attributes (CSI c) after it
    as a sentinel, and writes the raw reply to OUTDIR/xtversion.txt."""
    import select
    import termios
    import tty

    fd = sys.stdin.fileno()
    old = termios.tcgetattr(fd)
    tty.setraw(fd)
    reply = b""
    try:
        os.write(sys.stdout.fileno(), b"\x1b[>0q\x1b[c")
        end = time.monotonic() + 2
        while time.monotonic() < end and not (b"\x1b[?" in reply and reply.endswith(b"c")):
            if select.select([fd], [], [], 0.1)[0]:
                reply += os.read(fd, 1024)
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old)
    (out / "xtversion.txt").write_text(repr(reply))


def ghostty(out: Path) -> None:
    from PIL import Image

    out.mkdir(parents=True, exist_ok=True)
    subprocess.run(["ghostty", "--gtk-single-instance=false", "-e", sys.executable, "-I", os.path.abspath(__file__),
                    "query", str(out)], capture_output=True, timeout=20)
    print("XTVERSION reply:", (out / "xtversion.txt").read_text() if (out / "xtversion.txt").exists() else None)
    title = f"tbsrcrect{os.getpid()}"
    proc = subprocess.Popen(["ghostty", f"--title={title}", "--gtk-single-instance=false",
                             "--window-width=80", "--window-height=20", "-e",
                             sys.executable, "-I", os.path.abspath(__file__), "draw", str(out)])
    try:
        wid = subprocess.run(["xdotool", "search", "--sync", "--name", title], capture_output=True, text=True,
                             timeout=20).stdout.split()[0]
        for n in (1, 2, 3):
            flag = out / f"step-{n}"
            for _ in range(200):
                if flag.exists():
                    break
                time.sleep(0.05)
            subprocess.run(["xdotool", "windowactivate", "--sync", wid], capture_output=True)
            shot = out / f"step-{n}.png"
            subprocess.run(["import", "-window", wid, str(shot)], check=True)
            img = Image.open(shot).convert("RGB")
            counts = {name: 0 for name in QUADS}
            for px in img.get_flattened_data():
                for name, rgb in QUADS.items():
                    if all(abs(a - b) <= 8 for a, b in zip(px, rgb)):
                        counts[name] += 1
            print(f"step {n}: window {img.width}×{img.height} px; pixels of each colour: {counts}")
    finally:
        proc.wait(timeout=20)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "draw":
        draw(Path(sys.argv[2]))
    elif len(sys.argv) == 3 and sys.argv[1] == "query":
        query(Path(sys.argv[2]))
    elif len(sys.argv) == 3 and sys.argv[1] == "ghostty":
        ghostty(Path(sys.argv[2]))
    else:
        sys.exit(__doc__)
