#!/usr/bin/env python3
"""Right pane of the terminal finder.

Follows the file hovered in yazi (left pane) and draws a preview of it. On an
edit request it runs the editor right here, then hands focus back to yazi.

Usage: preview.py <run-dir>
  The run-dir's name is the yazi client id; it holds the fifo yazi writes
  "edit<TAB>path" / "open<TAB>path" / "quit" lines to.

Env:
  TFINDER_IMAGES      auto | sixel | symbols   (auto: sixel only for terminals known to support it)
  TFINDER_GLOW_STYLE  glow style for markdown previews (default: dark)
"""

from __future__ import annotations

import csv
import json
import os
import re
import selectors
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import termios
import time
import unicodedata
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

HOME = Path(__file__).resolve().parent

MD_EXT = {".md", ".markdown", ".mdown", ".mkd", ".mdx"}
CSV_EXT = {".csv", ".tsv"}
IMG_EXT = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tif", ".tiff", ".ico", ".avif", ".heic", ".jxl"}
HTML_EXT = {".html", ".htm", ".xhtml"}
HAS_SOURCE_VIEW = {"markdown", "html", "csv"}  # kinds where `s`/`i` flips rendered ⇄ source

MAX_LINES = 5000      # lines kept for scrolling
CSV_ROWS = 1000       # rows read into the CSV table
CSV_COL_MAX = 28      # widest a CSV column may get (cells are cut with …)
DEBOUNCE = 0.06       # wait this long after the last hover before drawing

ESC = "\x1b"
RESET, BOLD, DIM = f"{ESC}[0m", f"{ESC}[1m", f"{ESC}[2m"
BLUE, CYAN = f"{ESC}[1;34m", f"{ESC}[36m"


# ── small helpers ────────────────────────────────────────────────────────────

def char_width(ch: str) -> int:
    if unicodedata.combining(ch):
        return 0
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


def text_width(s: str) -> int:
    return sum(char_width(c) for c in s)


def fit(s: str, width: int, pad: bool = True) -> str:
    """Cut plain text to `width` terminal cells (ending in … when cut)."""
    if width <= 0:
        return ""
    w = text_width(s)
    if w <= width:
        return s + " " * (width - w) if pad else s
    out, used = [], 0
    for ch in s:
        cw = char_width(ch)
        if used + cw > width - 1:
            break
        out.append(ch)
        used += cw
    out.append("…")
    used += 1
    return "".join(out) + (" " * (width - used) if pad else "")


def wrap_plain(line: str, width: int) -> list[str]:
    if width <= 0 or text_width(line) <= width:
        return [line]
    rows, cur, used = [], [], 0
    for ch in line:
        cw = char_width(ch)
        if used + cw > width:
            rows.append("".join(cur))
            cur, used = [], 0
        cur.append(ch)
        used += cw
    rows.append("".join(cur))
    return rows


def human(n: float) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1024:
            return f"{n:.0f} {unit}" if unit == "B" else f"{n:.1f} {unit}"
        n /= 1024
    return f"{n:.1f} TB"


def run(cmd: list[str], env: dict | None = None, timeout: float = 10,
        input: str | None = None) -> str | None:
    """Run a renderer and return its stdout, or None if it failed outright."""
    stdin = {"input": input.encode()} if input is not None else {"stdin": subprocess.DEVNULL}
    try:
        r = subprocess.run(cmd, **stdin, capture_output=True, timeout=timeout,
                           env={**os.environ, **(env or {})})
    except (OSError, subprocess.TimeoutExpired):
        return None
    out = r.stdout.decode("utf-8", "replace")
    return out if (r.returncode == 0 or out.strip()) else None


def trim_blank(lines: list[str]) -> list[str]:
    while lines and not re.sub(r"\x1b\[[0-9;]*m", "", lines[-1]).strip():
        lines.pop()
    return lines


def count_lines(p: Path, limit: int = 50_000_000) -> int | None:
    try:
        if p.stat().st_size > limit:
            return None
        with open(p, "rb") as f:
            return sum(chunk.count(b"\n") for chunk in iter(lambda: f.read(1 << 20), b""))
    except OSError:
        return None


def looks_like_text(p: Path) -> bool:
    try:
        with open(p, "rb") as f:
            chunk = f.read(8192)
    except OSError:
        return False
    if b"\0" in chunk:
        return False
    try:
        chunk.decode("utf-8")
        return True
    except UnicodeDecodeError as e:
        return e.start >= len(chunk) - 3  # a character cut off at the end of the sample


def classify(p: Path) -> str:
    try:
        st = p.stat()
    except OSError:
        return "missing"
    if stat.S_ISDIR(st.st_mode):
        return "dir"
    if not stat.S_ISREG(st.st_mode):
        return "special"
    ext = p.suffix.lower()
    if ext in MD_EXT:
        return "markdown"
    if ext in HTML_EXT:
        return "html"
    if ext in CSV_EXT:
        return "csv"
    if ext in IMG_EXT:
        return "image"
    return "text" if looks_like_text(p) else "binary"


def image_mode() -> str:
    mode = os.environ.get("TFINDER_IMAGES", "auto").lower()
    if mode in ("sixel", "symbols"):
        return mode
    env = os.environ
    term_program = env.get("TERM_PROGRAM", "").lower()
    if (term_program in ("wezterm", "iterm.app", "mintty", "rio")
            or "WEZTERM_EXECUTABLE" in env or "KONSOLE_VERSION" in env
            or env.get("TERM", "").startswith(("foot", "mlterm", "contour"))):
        return "sixel"
    return "symbols"  # Ghostty, Terminator/GNOME Terminal (VTE), kitty, Alacritty …


# ── renderers: path → Preview ────────────────────────────────────────────────

@dataclass
class Preview:
    lines: list[str] = field(default_factory=list)
    info: str = ""
    image_cmd: list[str] | None = None  # sixel images are drawn straight to the terminal


def render_markdown(p: Path, cols: int, rows: int) -> Preview:
    n = count_lines(p)
    if shutil.which("glow"):
        style = os.environ.get("TFINDER_GLOW_STYLE", "dark")
        out = run(["glow", "-s", style, "-w", str(cols), str(p)], env={"CLICOLOR_FORCE": "1"})
        if out is not None:
            lines = trim_blank(out.split("\n"))
            while lines and not lines[0].strip():
                lines.pop(0)
            return Preview(lines[:MAX_LINES] or [f"{DIM}(empty file){RESET}"],
                           f"markdown · {n} lines" if n is not None else "markdown")
    return render_text(p, cols, rows)


def render_text(p: Path, cols: int, rows: int) -> Preview:
    n = count_lines(p)
    info = f"{n} lines" if n is not None else ""
    if n is not None and n > MAX_LINES:
        info += f" · showing first {MAX_LINES}"
    bat = shutil.which("batcat") or shutil.which("bat")
    if bat:
        out = run([bat, "--color=always", "--style=numbers", "--paging=never", "--wrap=character",
                   f"--terminal-width={cols}", f"--line-range=:{MAX_LINES}", "--", str(p)])
        if out is not None:
            return Preview(trim_blank(out.split("\n")) or [f"{DIM}(empty file){RESET}"], info)
    lines: list[str] = []
    with open(p, encoding="utf-8", errors="replace") as f:
        for i, line in enumerate(f):
            if i >= MAX_LINES:
                break
            lines.extend(wrap_plain(line.rstrip("\n").expandtabs(4), cols))
    return Preview(lines or [f"{DIM}(empty file){RESET}"], info)


def render_csv(p: Path, cols: int, rows: int) -> Preview:
    with open(p, newline="", encoding="utf-8", errors="replace") as f:
        sample = f.read(65536)
        f.seek(0)
        delim = "\t" if p.suffix.lower() == ".tsv" else ","
        if delim == ",":
            try:
                delim = csv.Sniffer().sniff(sample, delimiters=",;|\t").delimiter
            except csv.Error:
                pass
        data = []
        for i, row in enumerate(csv.reader(f, delimiter=delim)):
            if i > CSV_ROWS:
                break
            data.append([c.replace("\r", " ").replace("\n", " ").replace("\t", " ") for c in row])
    if not data:
        return Preview([f"{DIM}(empty file){RESET}"], "csv")

    ncols = max(len(r) for r in data)
    rows_ = [r + [""] * (ncols - len(r)) for r in data]
    header, body = rows_[0], rows_[1:]

    def numeric(col: int) -> bool:
        vals = [r[col].strip() for r in body if r[col].strip()]
        if not vals:
            return False
        try:
            [float(v.replace(",", "")) for v in vals[:200]]
            return True
        except ValueError:
            return False

    widths = [max(1, min(CSV_COL_MAX, max(text_width(r[c]) for r in rows_))) for c in range(ncols)]
    right = [numeric(c) for c in range(ncols)]

    # how many columns fit in the pane
    shown, used = 0, 1
    for w in widths:
        if used + w + 3 > cols and shown:
            break
        used += w + 3
        shown += 1
    widths, right = widths[:shown], right[:shown]

    def cell(text: str, c: int) -> str:
        t = fit(text, widths[c], pad=False)
        pad = " " * (widths[c] - text_width(t))
        return pad + t if right[c] else t + pad

    def line(r: list[str], bold: bool = False) -> str:
        sep = f"{DIM}│{RESET}"
        cells = [(f"{BOLD}{CYAN}{cell(r[c], c)}{RESET}" if bold else cell(r[c], c)) for c in range(shown)]
        return sep + sep.join(f" {x} " for x in cells) + sep

    def rule(l: str, m: str, r: str) -> str:
        return f"{DIM}{l}{m.join('─' * (w + 2) for w in widths)}{r}{RESET}"

    out = [rule("┌", "┬", "┐"), line(header, bold=True), rule("├", "┼", "┤")]
    out += [line(r) for r in body]
    out.append(rule("└", "┴", "┘"))

    total = count_lines(p)
    nrows = f"~{max(0, total - 1)} rows" if total is not None else f"{len(body)}+ rows"
    info = f"csv · {nrows} × {ncols} cols"
    if shown < ncols:
        info += f" · {ncols - shown} cols off-screen"
    if len(body) >= CSV_ROWS:
        info += f" · first {CSV_ROWS} rows"
    return Preview(out, info)


def html_title(p: Path) -> str:
    try:
        with open(p, encoding="utf-8", errors="replace") as f:
            head = f.read(65536)
    except OSError:
        return ""
    m = re.search(r"<title[^>]*>(.*?)</title>", head, re.I | re.S)
    if not m:
        return ""
    import html
    return " ".join(html.unescape(m.group(1)).split())


def html_headings(p: Path) -> dict[str, int]:
    """Heading text → level, so the (colourless) w3m dump can be styled."""
    import html
    try:
        src = p.read_text(encoding="utf-8", errors="replace")[:2_000_000]
    except OSError:
        return {}
    found = {}
    for level, inner in re.findall(r"<h([1-6])[^>]*>(.*?)</h\1\s*>", src, re.I | re.S):
        text = " ".join(html.unescape(re.sub(r"<[^>]+>", "", inner)).split())
        if text:
            found.setdefault(text, int(level))
    return found


def style_html_dump(lines: list[str], headings: dict[str, int]) -> list[str]:
    out, in_refs = [], False
    for line in lines:
        bare = " ".join(line.split())
        if bare == "References:":
            in_refs = True
        if in_refs:
            out.append(f"{DIM}{line}{RESET}")
        elif bare in headings:
            out.append(f"{BLUE if headings[bare] <= 2 else BOLD}{line}{RESET}")
        else:
            out.append(re.sub(r"\[(\d+)\]", f"{CYAN}[\\1]{RESET}", line))
    return out


def render_html(p: Path, cols: int, rows: int) -> Preview:
    """HTML → Markdown (html2text) → glow, so pages look like the .md previews.
    Falls back to w3m's text-browser dump, then to the source."""
    title = html_title(p)
    info = " · ".join(x for x in ("html", f"“{title}”" if title else "") if x)
    if shutil.which("html2markdown") and shutil.which("glow"):
        md = run(["html2markdown", "--body-width=0", "--unicode-snob", "--images-to-alt",
                  "--no-wrap-links", "--ignore-mailto-links", str(p)])
        if md is not None:
            style = os.environ.get("TFINDER_GLOW_STYLE", "dark")
            out = run(["glow", "-s", style, "-w", str(cols), "-"], input=md,
                      env={"CLICOLOR_FORCE": "1"})
            if out is not None:
                lines = trim_blank(out.split("\n"))
                while lines and not lines[0].strip():
                    lines.pop(0)
                return Preview(lines[:MAX_LINES] or [f"{DIM}(empty page){RESET}"], info)
    if shutil.which("w3m"):
        out = run(["w3m", "-dump", "-T", "text/html", "-cols", str(cols), "-O", "UTF-8",
                   "-o", "display_link_number=1", str(p)])
        if out is not None:
            lines = trim_blank(out.split("\n"))
            while lines and not lines[0].strip():
                lines.pop(0)
            return Preview(style_html_dump(lines[:MAX_LINES], html_headings(p))
                           or [f"{DIM}(empty page){RESET}"], info)
    pre = render_text(p, cols, rows)
    pre.info = info + " · source (install w3m to render)"
    return pre


def image_dims(p: Path) -> str:
    desc = run(["file", "-b", str(p)]) or ""
    m = re.search(r"(\d+)\s*x\s*(\d+)", desc)
    return f"{m.group(1)}×{m.group(2)}" if m else ""


def render_image(p: Path, cols: int, rows: int) -> Preview:
    info = " · ".join(x for x in ("image", image_dims(p)) if x)
    chafa = shutil.which("chafa")
    if not chafa:
        return Preview(["", "  (install chafa to preview images)"], info)
    if image_mode() == "sixel":
        return Preview([], info, image_cmd=[chafa, "-f", "sixels", "--size", f"{cols}x{rows}",
                                            "--animate", "off", str(p)])
    out = run([chafa, "-f", "symbols", "-c", "full", "--size", f"{cols}x{rows}",
               "--animate", "off", "--polite", "on", str(p)])
    if out is None:
        return Preview(["", "  (could not decode this image)"], info)
    return Preview(trim_blank(out.split("\n")), info)


def render_dir(p: Path, cols: int, rows: int) -> Preview:
    try:
        with os.scandir(p) as it:
            entries = list(it)
    except OSError as e:
        return Preview(["", f"  ({e.strerror.lower()})"], "folder")

    def is_dir(e: os.DirEntry) -> bool:
        try:
            return e.is_dir()
        except OSError:
            return False

    entries.sort(key=lambda e: (not is_dir(e), e.name.lower()))
    ndirs = sum(1 for e in entries if is_dir(e))
    lines = []
    for e in entries[:MAX_LINES]:
        d = is_dir(e)
        name = e.name + ("/" if d else "")
        size = ""
        if not d:
            try:
                size = human(e.stat().st_size)
            except OSError:
                pass
        style = BLUE if d else (DIM if e.name.startswith(".") else "")
        lines.append(f" {style}{fit(name, cols - 12)}{RESET}{DIM}{size:>10}{RESET}")
    info = f"folder · {ndirs} folders · {len(entries) - ndirs} files"
    return Preview(lines or [f"{DIM}  (empty folder){RESET}"], info)


def render_binary(p: Path, cols: int, rows: int) -> Preview:
    desc = (run(["file", "-b", str(p)]) or "binary file").strip()
    lines = ["", *(f"  {x}" for x in wrap_plain(desc, cols - 4)), "",
             f"  {DIM}No text preview. Press o to open it with a desktop app.{RESET}"]
    return Preview(lines, "binary")


RENDERERS = {
    "markdown": render_markdown, "text": render_text, "csv": render_csv, "html": render_html,
    "image": render_image, "dir": render_dir, "binary": render_binary,
}

WELCOME = [
    "",
    f"  {BOLD}Terminal Finder{RESET}",
    "",
    f"  {BOLD}Files pane (left){RESET}",
    "    ↑ ↓  j k        move — the file under the cursor shows up here",
    "    → ←  l h        open folder / go up",
    "    Enter  e        edit the file in this pane",
    "    o               open with the desktop app (html → your browser)",
    "    i               rendered ⇄ source view (markdown, html, csv)",
    "    q               close the finder",
    "",
    f"  {BOLD}Preview pane (here){RESET}",
    "    ↑ ↓  j k        scroll            space b  page down / up",
    "    g  G            top / bottom      e  edit    o  open",
    "    h  Esc          back to the files pane",
    "",
    f"  {BOLD}Mouse{RESET}             click folder → open · click file → preview",
    "                    double-click / right-click file → edit · wheel scrolls",
    "                    click empty space in the left column → go up",
    "",
    f"  {BOLD}Switch panes{RESET}      Alt+h / Alt+l   (or Alt+← / Alt+→)",
    "",
    f"  {BOLD}Editing{RESET}",
    "    text, markdown  Neovim: just type · Alt+s save · Alt+q close",
    "    csv             VisiData: e edit cell · Alt+s save · q close",
    "    images          opens in a desktop app",
    "",
    f"  {DIM}Careful: Ctrl+Q quits all of zellij in your setup.{RESET}",
]


# ── the pane ─────────────────────────────────────────────────────────────────

KEYS = {
    "j": ("scroll", 1), f"{ESC}[B": ("scroll", 1), "k": ("scroll", -1), f"{ESC}[A": ("scroll", -1),
    " ": ("page", 1), "f": ("page", 1), f"{ESC}[6~": ("page", 1), "d": ("half", 1),
    "b": ("page", -1), f"{ESC}[5~": ("page", -1), "u": ("half", -1),
    "g": ("top", 0), f"{ESC}[H": ("top", 0), f"{ESC}[1~": ("top", 0),
    "G": ("bottom", 0), f"{ESC}[F": ("bottom", 0), f"{ESC}[4~": ("bottom", 0),
    "e": ("edit", 0), "\r": ("edit", 0), "\n": ("edit", 0), "o": ("open", 0),
    "s": ("source", 0), "i": ("source", 0),
    "h": ("back", 0), "q": ("back", 0), ESC: ("back", 0), f"{ESC}[D": ("back", 0), "\x03": ("back", 0),
}


def key_tokens(data: str):
    i = 0
    while i < len(data):
        if data[i] == ESC and i + 1 < len(data) and data[i + 1] in "[O":
            j = i + 2
            while j < len(data) and not ("@" <= data[j] <= "~"):
                j += 1
            seq = data[i:j + 1]
            i = j + 1
            yield f"{ESC}[" + seq[2:] if seq.startswith(f"{ESC}O") else seq  # SS3 arrows → CSI
        else:
            yield data[i]
            i += 1


def zellij(*args: str) -> None:
    if os.environ.get("ZELLIJ") is None:
        return
    try:
        subprocess.run(["zellij", "action", *args], stdin=subprocess.DEVNULL,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=3)
    except (OSError, subprocess.TimeoutExpired):
        pass


class Pane:
    def __init__(self, run_dir: Path):
        self.run_dir = run_dir
        self.client_id = run_dir.name
        self.tty = sys.stdin.fileno()
        self.orig_tty = termios.tcgetattr(self.tty)
        self.sel = selectors.DefaultSelector()

        self.path: Path | None = None
        self.cwd: str | None = None
        self.kind = ""
        self.source_view = False
        self.cache: dict[tuple, Preview] = {}
        self.signature = None
        self.preview = Preview(WELCOME, "")
        self.top = 0
        self.pending: str | None = None
        self.pending_at = 0.0
        self.resized = False

        fifo = run_dir / "fifo"
        if not fifo.exists():
            run_dir.mkdir(parents=True, exist_ok=True)
            os.mkfifo(fifo)
        # O_RDWR: we also count as a writer, so the fifo never reports EOF.
        self.fifo_fd = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
        self.fifo_buf = b""
        self.sel.register(self.fifo_fd, selectors.EVENT_READ, "fifo")
        self.sel.register(self.tty, selectors.EVENT_READ, "keys")

        self.sub: subprocess.Popen | None = None
        self.sub_buf = b""
        self.sub_retry_at = 0.0

    # terminal state ---------------------------------------------------------

    def screen_on(self) -> None:
        a = termios.tcgetattr(self.tty)
        a[0] &= ~(termios.IXON | termios.ICRNL)
        a[3] &= ~(termios.ECHO | termios.ICANON | termios.ISIG | termios.IEXTEN)
        a[6][termios.VMIN], a[6][termios.VTIME] = 1, 0
        termios.tcsetattr(self.tty, termios.TCSANOW, a)
        self.write(f"{ESC}[?1049h{ESC}[?25l{ESC}[?7l")  # alt screen, hide cursor, no autowrap

    def screen_off(self) -> None:
        self.write(f"{ESC}[0m{ESC}[2J{ESC}[?7h{ESC}[?25h{ESC}[?1049l")
        termios.tcsetattr(self.tty, termios.TCSANOW, self.orig_tty)

    def write(self, s: str) -> None:
        b = s.encode("utf-8", "replace")
        while b:
            b = b[os.write(1, b):]

    def size(self) -> tuple[int, int]:
        try:
            sz = os.get_terminal_size(1)
            return max(3, sz.lines), max(10, sz.columns)
        except OSError:
            return 24, 80

    # yazi hover events ------------------------------------------------------

    def start_sub(self) -> None:
        try:
            self.sub = subprocess.Popen(["ya", "sub", "hover,cd"], stdin=subprocess.DEVNULL,
                                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        except OSError:
            self.sub, self.sub_retry_at = None, time.monotonic() + 5
            return
        os.set_blocking(self.sub.stdout.fileno(), False)
        self.sel.register(self.sub.stdout, selectors.EVENT_READ, "sub")

    def read_sub(self) -> None:
        try:
            data = self.sub.stdout.read()
        except (BlockingIOError, ValueError):
            return
        if not data:  # ya sub exited (e.g. yazi wasn't up yet) → retry shortly
            self.sel.unregister(self.sub.stdout)
            self.sub.wait()
            self.sub, self.sub_retry_at = None, time.monotonic() + 0.5
            return
        self.sub_buf += data
        *lines, self.sub_buf = self.sub_buf.split(b"\n")
        for raw in lines:
            parts = raw.decode("utf-8", "replace").split(",", 3)
            if len(parts) != 4 or parts[2] != self.client_id:
                continue  # another yazi instance
            try:
                url = json.loads(parts[3]).get("url")
            except (ValueError, AttributeError):
                continue
            if parts[0] == "cd":
                self.cwd = url
            elif parts[0] == "hover":
                url = url or self.cwd  # nothing hovered = empty folder → show the folder
                if url:
                    self.pending, self.pending_at = url, time.monotonic()

    # requests from yazi -----------------------------------------------------

    def read_fifo(self) -> None:
        try:
            self.fifo_buf += os.read(self.fifo_fd, 65536)
        except BlockingIOError:
            return
        *lines, self.fifo_buf = self.fifo_buf.split(b"\n")
        for raw in lines:
            action, _, arg = raw.decode("utf-8", "surrogateescape").partition("\t")
            if action == "quit":
                raise SystemExit(0)
            if action == "edit" and arg:
                self.edit(Path(arg))
            elif action == "open" and arg:
                self.open_external(Path(arg))
            elif action == "toggle":
                self.toggle_source()

    # drawing ----------------------------------------------------------------

    def show(self, path: Path, keep_scroll: bool = False) -> None:
        rows, cols = self.size()
        try:
            st = path.stat()
            sig = (str(path), st.st_mtime_ns, st.st_size, rows, cols)
        except OSError:
            sig = (str(path), None, None, rows, cols)
        if sig == self.signature and not keep_scroll:
            return  # same file, unchanged: yazi often repeats hover events
        self.signature = sig
        self.path = path
        kind = classify(path)
        self.kind = kind
        cache_key = (*sig, self.source_view)
        if cache_key in self.cache:  # big HTML pages take ~1s to convert; revisits are instant
            self.preview = self.cache[cache_key]
            if not keep_scroll:
                self.top = 0
            self.draw(full=True)
            return
        try:
            if self.source_view and kind in HAS_SOURCE_VIEW:
                self.preview = render_text(path, cols, rows - 2)
                self.preview.info = f"{kind} source · {self.preview.info}".rstrip(" ·")
            elif kind in RENDERERS:
                self.preview = RENDERERS[kind](path, cols, rows - 2)
            elif kind == "missing":
                self.preview = Preview(["", f"  {DIM}(not found){RESET}"])
            else:
                self.preview = Preview(["", f"  {DIM}(special file){RESET}"])
        except Exception as e:  # a broken file must never kill the pane
            self.preview = Preview(["", f"  preview failed: {e}"])
        if sig[1] is not None and kind != "dir" and not self.preview.image_cmd:
            self.cache[cache_key] = self.preview
            while len(self.cache) > 40:
                self.cache.pop(next(iter(self.cache)))
        if not keep_scroll:
            self.top = 0
        self.draw(full=True)

    def header(self, cols: int) -> str:
        if self.path is None:
            return ""
        try:
            st = self.path.stat()
            meta = [] if stat.S_ISDIR(st.st_mode) else [human(st.st_size)]
            meta.append(datetime.fromtimestamp(st.st_mtime).strftime("%Y-%m-%d %H:%M"))
        except OSError:
            meta = []
        info = "  ·  ".join(x for x in (self.preview.info, *meta) if x)
        name = self.path.name or str(self.path)
        name_txt = fit(f" {name} ", min(cols, max(12, cols - text_width(info) - 2)), pad=False)
        rest = fit(f"  {info}", cols - text_width(name_txt), pad=True)
        return f"{ESC}[7m{BOLD}{name_txt}{ESC}[22m{rest}{RESET}"

    def footer(self, cols: int, body: int) -> str:
        n = len(self.preview.lines)
        pos = ""
        if not self.preview.image_cmd and n > body:
            pos = f"{self.top + 1}–{min(self.top + body, n)}/{n} "
        keys = " e edit · o open · ↑↓ scroll · Alt+h files" if self.path else " Alt+h files"
        if self.path and self.kind in HAS_SOURCE_VIEW:
            keys += " · i " + ("rendered" if self.source_view else "source")
        keys = fit(keys, max(0, cols - len(pos)), pad=True)
        return f"{DIM}{keys}{pos}{RESET}"

    def draw(self, full: bool = False) -> None:
        rows, cols = self.size()
        body = rows - 2
        buf = [f"{ESC}[0m{ESC}[2J" if full else "", f"{ESC}[1;1H{ESC}[2K{self.header(cols)}"]
        if self.preview.image_cmd:
            self.write("".join(buf) + f"{ESC}[{rows};1H{ESC}[2K{self.footer(cols, body)}{ESC}[2;1H")
            try:
                subprocess.run(self.preview.image_cmd, stdin=subprocess.DEVNULL, timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                pass
            return
        lines = self.preview.lines
        for i in range(body):
            idx = self.top + i
            text = lines[idx] if idx < len(lines) else ""
            buf.append(f"{ESC}[{i + 2};1H{ESC}[0m{ESC}[2K{text}{RESET}")
        buf.append(f"{ESC}[{rows};1H{ESC}[2K{self.footer(cols, body)}")
        self.write("".join(buf))

    def scroll_to(self, top: int) -> None:
        body = self.size()[0] - 2
        top = max(0, min(top, len(self.preview.lines) - body))
        if top != self.top:
            self.top = top
            self.draw()

    # actions ----------------------------------------------------------------

    def key(self, token: str) -> None:
        action, n = KEYS.get(token, (None, 0))
        body = self.size()[0] - 2
        if action == "scroll":
            self.scroll_to(self.top + n)
        elif action == "page":
            self.scroll_to(self.top + n * max(1, body - 1))
        elif action == "half":
            self.scroll_to(self.top + n * max(1, body // 2))
        elif action == "top":
            self.scroll_to(0)
        elif action == "bottom":
            self.scroll_to(len(self.preview.lines))
        elif action == "edit" and self.path:
            self.edit(self.path)
        elif action == "open" and self.path:
            self.open_external(self.path)
        elif action == "source":
            self.toggle_source()
        elif action == "back":
            self.focus_files()

    def toggle_source(self) -> None:
        """Rendered ⇄ source view. Sticks while you browse, so you can flip through sources."""
        self.source_view = not self.source_view
        if self.path:
            self.signature = None
            self.show(self.path)

    def focus_files(self) -> None:
        try:
            pane_id = (self.run_dir / "files.pane").read_text().strip()
        except OSError:
            pane_id = ""
        if pane_id:
            zellij("focus-pane-id", pane_id)
        else:
            zellij("move-focus", "left")

    def open_external(self, path: Path, edit: bool = False) -> None:
        cmd = ["xdg-open", str(path)]
        if edit:
            for app in ("pinta", "gimp", "krita"):
                if shutil.which(app):
                    cmd = [app, str(path)]
                    break
        try:
            subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL, start_new_session=True)
        except OSError:
            pass

    def editor_cmd(self, path: Path, kind: str) -> tuple[list[str], dict]:
        env = dict(os.environ)
        if kind == "csv" and shutil.which("vd"):
            return ["vd", "--config", str(HOME / "visidata" / "visidatarc"), str(path)], env
        nvim = shutil.which("nvim") or str(Path.home() / ".local/bin/nvim")
        if os.access(nvim, os.X_OK):
            env["NVIM_APPNAME"] = "tfinder-nvim"
            return [nvim, "-u", str(HOME / "nvim" / "init.lua"), str(path)], env
        fallback = os.environ.get("EDITOR") or shutil.which("nano") or "vi"
        return [*shlex.split(fallback), str(path)], env

    def edit(self, path: Path) -> None:
        kind = classify(path)
        if kind in ("dir", "missing", "special"):
            return
        if kind in ("image", "binary"):
            self.open_external(path, edit=(kind == "image"))
            return
        cmd, env = self.editor_cmd(path, kind)
        zellij("focus-pane-id", os.environ.get("ZELLIJ_PANE_ID", ""))
        self.screen_off()
        termios.tcflush(self.tty, termios.TCIFLUSH)  # drop keys typed into the preview
        try:
            subprocess.run(cmd, env=env)
        except OSError as e:
            print(f"could not start editor: {e}")
            time.sleep(2)
        finally:
            self.screen_on()
            termios.tcflush(self.tty, termios.TCIFLUSH)
        self.focus_files()
        self.signature = None
        self.show(path, keep_scroll=(path == self.path))

    # main loop --------------------------------------------------------------

    def loop(self) -> None:
        self.screen_on()
        self.draw(full=True)
        while True:
            now = time.monotonic()
            if self.sub is None and now >= self.sub_retry_at:
                self.start_sub()
            timeout = 0.2
            if self.pending:
                timeout = max(0.0, DEBOUNCE - (now - self.pending_at))
            for key, _ in self.sel.select(timeout):
                if key.data == "sub":
                    self.read_sub()
                elif key.data == "fifo":
                    self.read_fifo()
                elif key.data == "keys":
                    data = os.read(self.tty, 4096).decode("utf-8", "replace")
                    for token in key_tokens(data):
                        self.key(token)
            if self.pending and time.monotonic() - self.pending_at >= DEBOUNCE:
                path, self.pending = self.pending, None
                self.show(Path(path))
            if self.resized:
                self.resized = False
                if self.path:
                    self.signature = None
                    self.show(self.path, keep_scroll=True)
                else:
                    self.draw(full=True)

    def close(self) -> None:
        try:
            self.screen_off()
        except (OSError, termios.error):
            pass
        if self.sub:
            self.sub.kill()
        shutil.rmtree(self.run_dir, ignore_errors=True)


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    if not sys.stdin.isatty():
        sys.exit("preview.py must run in a terminal pane")
    pane = Pane(Path(sys.argv[1]))

    def on_resize(*_):
        pane.resized = True

    def on_exit(*_):
        raise SystemExit(0)

    signal.signal(signal.SIGWINCH, on_resize)
    signal.signal(signal.SIGTERM, on_exit)
    signal.signal(signal.SIGHUP, on_exit)
    try:
        pane.loop()
    except Exception:
        pane.close()
        import traceback
        traceback.print_exc()
        input("\npreview pane crashed (see above). Press Enter to close it.")
    finally:
        pane.close()


if __name__ == "__main__":
    main()
