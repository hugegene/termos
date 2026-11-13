#!/usr/bin/env python3
"""Records the demo for the README, an animated WebP: tos in Ghostty on a virtual display, driven by key presses.

    cargo build --release
    python3 scripts/demo/record.py [--tos target/release/tos] [--out FILE.webp|FILE.gif ...] [--no-claude]
                                   [--terminal ghostty|xterm] [--video FILE] [--image FILE]

By default it writes docs/images/finder-demo.webp (lossless) and docs/images/finder-demo.gif (256 colours, so the
pictures band a little), from one recording. --video and --image show your own video and picture instead of the
made-up ones (see make_files.py); the README's demo uses a video and a frame from ~/ghibli-video.

The tour: `tos attach` from a plain shell and a finder tab; a new tab where Claude Code writes a small web
server; back in the finder, `/` finds it; then a walk through every kind of file. `--no-claude` leaves Claude out.

Needs Xvfb, Ghostty (or xterm with sixel, as Ubuntu's is), xdotool, ffmpeg, LibreOffice and Python with Pillow,
openpyxl and matplotlib; for the Claude part, Claude Code signed in. Nothing touches your own sessions or files:
tos and Claude run with their own HOME, sockets, state and Claude config in a folder under /tmp, and Ghostty with
its own D-Bus; all of it is ended and removed afterwards. Plan: docs/plans/2026-11-13-finder-demo-gif.md.

    ┌ Xvfb :N (virtual display) ───────────────────┐
    │  Ghostty (kitty images), own D-Bus           │  1 key presses   ┌──────────┐
    │   └ bash ── tos attach ── tos server ◀───────┼──────────────────│ this     │
    │   tabs: finder · bash · claude               │  (xdotool)       │ script   │
    └──────────────────────────────────────────────┘                  └──────────┘
          │ 2 frames (ffmpeg x11grab)                                       │ 3 captions and fast-forwards,
          ▼                                                                 ▼   with their times
       raw.mkv ──────────── 4 ffmpeg: captions, speed, lossless WebP ──▶ finder-demo.webp
"""

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from contextlib import contextmanager
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent

COLS, ROWS = 104, 34  # 8×17-pixel cells: 832×578, about the width GitHub shows a README image at
FPS = 15
CAPTION_H = 40
CLAUDE_PROMPT = 'Write a simple web server in server.py that says "Hello from the bakery!"'
CLAUDE_SHOWN = 6.0  # seconds that Claude's work takes in the full animation, at most (it's sped up to fit)
CLAUDE_SHOWN_SHORT = 2.0  # and in the short one

# Catppuccin Mocha: background, foreground and the 16 ANSI colours.
BG, FG = "#1e1e2e", "#cdd6f4"
ANSI = ["#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
        "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8"]

# Keys of ~/.claude.json that say who you are or what you work on; the rest (notices and tips already seen) is
# copied so Claude starts without first-run notices.
CLAUDE_PRIVATE = {"oauthAccount", "userID", "machineID", "projects", "githubRepoPaths", "skillUsage", "pluginUsage",
                  "agentLastUsed", "feedbackSurveyState", "feedbackDraftsTurnOffPromptDeclines",
                  "replBridgePlaceholders"}


class Demo:
    def __init__(self, env, t0, tos):
        self.env = env
        self.t0 = t0
        self.tos = tos
        self.captions = []  # (start, text)
        self.fast = []  # (start, end, factor): stretches shown sped up
        self.snaps = []  # (time, weight): frames the GIF's palette is made from
        self.short = []  # (start, end, factor): the stretches the short animation is cut from, in order
        self.factors = {"full": 1, "short": 1}  # how much Claude's work is sped up in each, for its caption

    def highlight(self, seconds, factor=1):
        """Marks the next `seconds` of the recording for the short animation, at `factor` times the speed. The
        tour goes on as it would; the mark only says what to cut."""
        start = self.now()
        self.short.append((start, start + seconds, factor))

    @contextmanager
    def highlighting(self, factor):
        """Marks everything done inside the `with` for the short animation, sped up `factor` times."""
        start = self.now()
        yield
        self.short.append((start, self.now(), factor))

    def snap(self, weight=1):
        """Marks this moment's frame for the palette: the pictures, so they get colours enough not to band,
        and a few screens of text. A smooth gradient needs more colours, so it counts `weight` times."""
        self.snaps.append((self.now(), weight))

    def now(self):
        return time.monotonic() - self.t0

    def caption(self, text, pause=0.9):
        """Shows `text` under the terminal from now until the next caption, then waits for it to be read.
        `keys` in backquotes are drawn as keys."""
        self.captions.append((self.now(), text))
        time.sleep(pause)

    def xdo(self, *args):
        """Through XTEST, to the focused window: xterm ignores events sent to a window (allowSendEvents)."""
        subprocess.run(["xdotool", *args], env=self.env, check=True)

    def key(self, *keys, wait=0.6, gap=0.12):
        for k in keys:
            self.xdo("key", k)
            time.sleep(gap)
        time.sleep(wait)

    def type(self, text, wait=0.4):
        self.xdo("type", "--delay", "70", text)
        time.sleep(wait)

    def screen(self):
        """The focused pane's text."""
        return subprocess.run([str(self.tos), "dump-screen"], env=self.env, capture_output=True, text=True).stdout

    def wait_screen(self, cond, what, timeout):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if cond(self.screen()):
                return
            time.sleep(0.3)
        raise RuntimeError(f"timed out after {timeout:.0f} s waiting for {what}")

    def wait_for_claude(self, made, timeout=300):
        """Waits until Claude has written `made` and stopped: no "esc to interrupt", and the screen still for 1.5 s
        (the spinner and its timer change it every second while it works). Shown sped up, and the caption from
        just before says by how much."""
        start = self.now()
        end = time.monotonic() + timeout
        last = None
        while True:
            if time.monotonic() > end:
                raise RuntimeError(f"Claude hadn't finished after {timeout} s")
            time.sleep(1.5)
            screen = self.screen()
            if made.exists() and "esc to interrupt" not in screen and screen == last:
                break
            last = screen
        if "Esc to cancel" in screen:  # a question Claude asks afterwards
            self.key("Escape", wait=0.5)
        took = self.now() - start
        self.factors = {"full": max(2, round(took / CLAUDE_SHOWN)), "short": max(4, round(took / CLAUDE_SHOWN_SHORT))}
        self.fast.append((start, self.now(), self.factors["full"]))
        self.short.append((start, self.now(), self.factors["short"]))


def start(d):
    """`tos attach` from a plain shell, then a finder tab on the sample folder."""
    d.caption("In your terminal, `tos attach` starts a session, or goes back to the one running", pause=1.0)
    d.type("tos attach")
    d.key("Return", wait=1.4)
    d.caption("`tos finder` opens a finder tab on this folder", pause=0.4)
    d.type("tos finder")
    d.key("Return", wait=0.6)
    d.highlight(1.6)
    time.sleep(1.6)


def tour(d, server):
    """A walk through every file, from the top. The folder lists budget.xlsx, index.html, the video (mandelbrot.mp4
    or movie.mp4), notes.docx, README.md, report.pdf, sales.csv, server.py (when Claude wrote it) and the picture
    (sunset.png or snapshot.png), in that order."""
    d.highlight(1.8)
    d.caption("`↓` `↑` move through the files · spreadsheets show as a table", pause=0.9)
    d.snap()
    time.sleep(0.9)
    d.caption("`→` focuses the preview · `]` shows the next sheet")
    d.key("Right", wait=0.4)
    d.key("bracketright", wait=1.8)
    d.key("bracketleft", "Left", wait=0.5)

    d.highlight(1.4)
    d.key("Down", wait=0.2)
    d.caption("HTML shows as formatted text", pause=1.8)
    with d.highlighting(3):
        d.caption("`→` `Enter` edits it in Neovim, inside the preview")
        d.key("Right", "Return", wait=1.4)
        d.key(*["Down"] * 10, gap=0.08, wait=0.3)
        d.key("End", "Return", wait=0.2)
        d.type("<li>Rye loaf</li>", wait=0.7)
        d.caption("`Ctrl+s` saves · `Ctrl+e` closes · the preview shows the change")
        d.key("ctrl+s", wait=0.7)
    d.highlight(1.6)
    d.key("ctrl+e", wait=2.2)
    d.key("Left", wait=0.2)

    d.highlight(1.8)
    d.key("Down", wait=0.2)
    d.caption("Videos show their length, size and a frame", pause=1.2)
    d.snap(weight=2)
    time.sleep(1.2)

    d.highlight(1.4)
    d.key("Down", wait=0.2)
    d.caption("Word documents show as Markdown", pause=2.2)

    d.key("Down", wait=0.2)
    d.caption("Markdown is rendered", pause=1.8)
    d.caption("`→` `Enter` edits it: tick a box")
    d.key("Right", "Return", wait=1.3)
    d.key(*["Down"] * 16, gap=0.06, wait=0.3)
    d.key("Home", "Right", "Right", "Right", "Delete", wait=0.2)
    d.type("x", wait=0.7)
    d.key("ctrl+s", wait=0.5)
    d.key("ctrl+e", wait=2.0)
    d.key("Left", wait=0.2)

    d.key("Down", wait=0.2)
    d.caption("PDFs show their text", pause=1.8)
    d.caption("`t` shows the pages as images · `]` next page")
    d.key("Right", wait=0.3)
    d.highlight(2.2)
    d.key("t", wait=1.5)
    d.snap()
    time.sleep(0.9)
    d.key("bracketright", wait=2.2)
    d.key("t", wait=0.4)
    d.key("Left", wait=0.2)

    d.key("Down", wait=0.2)
    d.caption("CSV shows as a table", pause=1.8)
    with d.highlighting(3):
        d.caption("`→` `Enter` opens the grid editor · `e` edits a cell")
        d.key("Right", "Return", wait=1.0)
        d.key(*["Down"] * 12, gap=0.08, wait=0.2)
        d.key("Right", wait=0.5)
        d.key("e", wait=0.5)
        d.key("BackSpace", "BackSpace", "BackSpace", gap=0.15, wait=0.2)
        d.type("530", wait=0.3)
        d.key("Return", wait=0.7)
    d.caption("`Ctrl+s` saves · `Ctrl+e` closes the grid")
    d.key("ctrl+s", wait=1.0)
    d.key("ctrl+e", wait=1.8)
    d.key("Left", wait=0.2)

    if server:
        d.key("Down", wait=0.2)
        d.caption("Code shows highlighted: Claude's server.py", pause=1.4)

    d.highlight(2.2)
    d.key("Down", wait=0.2)
    d.caption("Images show for real in kitty, iTerm2, WezTerm, Ghostty or xterm", pause=1.5)
    d.snap(weight=3)
    time.sleep(1.9)


def claude(d, folder):
    """A new tab, Claude Code writing server.py there, then back to the finder, where `/` finds server.py and Esc
    clears the filter, leaving the first file selected for the tour. A caption's {factor} is filled in for each
    animation, as each speeds Claude up differently."""
    with d.highlighting(3):
        d.caption("`Ctrl+t` `n` opens a new tab")
        d.key("ctrl+t", gap=0.3, wait=0)
        d.key("n", wait=1.0)
        d.caption("Run Claude Code in it", pause=0.3)
        d.type("claude")
        d.key("Return", wait=0.5)
        d.wait_screen(lambda s: "❯" in s, "Claude's prompt", timeout=30)
        time.sleep(1.2)
    with d.highlighting(4):
        d.caption("Ask it for a simple web server", pause=0.3)
        d.type(CLAUDE_PROMPT, wait=0.8)
        d.key("Return", wait=0)
    d.caption("Claude writes server.py (sped up {factor}×)", pause=0)
    d.wait_for_claude(folder / "server.py")
    d.highlight(2.8)
    d.caption("Done: Claude's reply", pause=0.5)
    d.snap()
    time.sleep(4.0)
    d.highlight(2.6)
    d.caption("`Ctrl+t` `Tab` goes back to the finder")
    d.key("ctrl+t", gap=0.3, wait=0)
    d.key("Tab", wait=0.2)
    d.key("Escape", wait=1.4)
    with d.highlighting(2):
        d.caption("`/` filters: type part of a name", pause=0.3)
        d.type("/server", wait=0.6)
        d.key("Return", wait=0.3)
    d.highlight(3.2)
    d.caption("There's server.py, highlighted · `Esc` clears the filter", pause=1.0)
    d.snap()
    time.sleep(2.6)
    d.key("Escape", wait=0.8)


def free_display():
    for n in range(87, 120):
        if not Path(f"/tmp/.X11-unix/X{n}").exists() and not Path(f"/tmp/.X{n}-lock").exists():
            return f":{n}"
    sys.exit("no free X display between :87 and :119")


def wait_for(cond, what, timeout=10.0):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if cond():
            return
        time.sleep(0.1)
    sys.exit(f"timed out waiting for {what}")


def claude_config(work, folder):
    """A Claude config folder of our own: your login, the notices you've already seen, and the sample folder
    trusted. The login's token must last the whole recording: a copy that refreshed it would sign out the
    original."""
    own = os.environ.get("CLAUDE_CONFIG_DIR")
    src = Path(own) if own else Path.home() / ".claude"
    creds = src / ".credentials.json"
    if not creds.is_file():
        sys.exit(f"no Claude login in {creds}; sign in to Claude Code, or use --no-claude")
    expires = json.loads(creds.read_text()).get("claudeAiOauth", {}).get("expiresAt", 0) / 1000
    if expires - time.time() < 30 * 60:
        sys.exit("your Claude login's token expires within 30 minutes; use Claude Code once to renew it, then run "
                 "this again")
    seen_path = src / ".claude.json" if own else Path.home() / ".claude.json"
    seen = json.loads(seen_path.read_text()) if seen_path.is_file() else {}
    cfg = {k: v for k, v in seen.items() if k not in CLAUDE_PRIVATE}
    cfg.update(hasCompletedOnboarding=True, autoUpdates=False,
               projects={str(folder): {"hasTrustDialogAccepted": True}})
    cfg.setdefault("theme", "dark")
    # "Teach auto mode about your environment?" after the first task.
    cfg["autoModeEnvSetup"] = {**cfg.get("autoModeEnvSetup", {}), "dismissed": True}
    dest = work / "claude"
    dest.mkdir(mode=0o700)
    shutil.copy(creds, dest / ".credentials.json")
    os.chmod(dest / ".credentials.json", 0o600)
    (dest / ".claude.json").write_text(json.dumps(cfg))
    return dest


def kill_leftovers(work, marker):
    """Ends every process started for the demo: those with `marker` in their environment (every child of the
    terminal, and the D-Bus services Ghostty starts) or working in the folder (such as a server Claude started).
    Then unmounts anything mounted in the folder, such as gvfs's."""
    for sig in (signal.SIGTERM, signal.SIGKILL):
        found = False
        for proc in Path("/proc").iterdir():
            if not proc.name.isdigit() or int(proc.name) == os.getpid():
                continue
            try:
                ours = marker in (proc / "environ").read_bytes().split(b"\0")
                if ours or os.readlink(proc / "cwd").startswith(str(work)):
                    os.kill(int(proc.name), sig)
                    found = True
            except (OSError, ValueError):
                pass
        if not found:
            break
        time.sleep(1)
    for line in Path("/proc/mounts").read_text().splitlines():
        point = line.split()[1]
        if point.startswith(str(work)):
            subprocess.run(["fusermount", "-u", point], stderr=subprocess.DEVNULL)


def terminal_command(name, folder):
    """The terminal, COLS×ROWS cells of 8×17 pixels, Catppuccin Mocha, running bash in `folder`."""
    if name == "ghostty":
        # Its own D-Bus: Ghostty is a GTK app, and without a bus it starts one that outlives it. No single
        # instance, or it would hand the window to a Ghostty you have open.
        return ["dbus-run-session", "--", "ghostty", "--config-default-files=false", "--gtk-single-instance=false",
                "--window-decoration=false", "--theme=Catppuccin Mocha", "--font-size=10",
                f"--window-width={COLS}", f"--window-height={ROWS}", "--cursor-style-blink=false",
                "--resize-overlay=never", "--confirm-close-surface=false", "--shell-integration=none",
                f"--working-directory={folder}"]
    xrm = [f"XTerm*color{i}: {c}" for i, c in enumerate(ANSI)] + [
        "XTerm*numColorRegisters: 256",  # the default 16 garbles sixel colours (findings/2026-11-05-xterm-sixel.md)
        "XTerm*maxGraphicSize: 2000x2000",
        "XTerm*metaSendsEscape: true",  # Alt+s as Esc s, as other terminals send it
        "XTerm*cursorColor: " + FG,
    ]
    cmd = ["xterm", "-ti", "vt340", "-u8", "-fa", "DejaVu Sans Mono", "-fs", "10",
           "-geometry", f"{COLS}x{ROWS}+0+0", "-bg", BG, "-fg", FG, "-b", "0", "-bw", "0", "+sb"]
    for r in xrm:
        cmd += ["-xrm", r]
    return cmd


def render_captions(captions, width, out_dir):
    """One PNG per caption: the text, with `keys` drawn as keys."""
    from PIL import Image, ImageDraw, ImageFont

    sans = ImageFont.truetype("DejaVuSans.ttf", 15)
    mono = ImageFont.truetype("DejaVuSansMono-Bold.ttf", 14)
    out_dir.mkdir(exist_ok=True)
    paths = []
    for i, (_, text) in enumerate(captions):
        img = Image.new("RGB", (width, CAPTION_H), "#11111b")
        d = ImageDraw.Draw(img)
        x, mid = 14, CAPTION_H // 2
        for j, part in enumerate(text.split("`")):
            if not part:
                continue
            if j % 2:  # a key
                w = d.textlength(part, font=mono)
                d.rounded_rectangle((x, mid - 12, x + w + 14, mid + 12), radius=5, fill="#313244", outline="#6c7086")
                d.text((x + 7, mid), part, font=mono, fill="#f9e2af", anchor="lm")
                x += w + 14 + 4
            else:
                d.text((x, mid), part, font=sans, fill=FG, anchor="lm")
                x += d.textlength(part, font=sans)
        p = out_dir / f"caption{i:02}.png"
        img.save(p)
        paths.append(p)
    return paths


def make_palette(raw, snaps, caption_pngs, work):
    """The GIF's 256 colours, from the marked frames and the captions. A palette from every frame gives the
    pictures too few colours, as screens of text outnumber them, and their gradients band; dithering harder
    instead speckles the background (findings/2026-11-13-ghostty-demo.md)."""
    from PIL import Image

    parts = []
    for i, (t, weight) in enumerate(snaps):
        p = work / f"snap{i:02}.png"
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-ss", f"{t:.2f}", "-i", str(raw), "-frames:v", "1", str(p)],
                       check=True)
        parts += [Image.open(p).convert("RGB")] * weight
    parts += [Image.open(p).convert("RGB") for p in caption_pngs]
    sheet = Image.new("RGB", (max(im.width for im in parts), sum(im.height for im in parts)))
    y = 0
    for im in parts:
        sheet.paste(im, (0, y))
        y += im.height
    sheet.save(work / "palette-source.png")
    palette = work / "palette.png"
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(work / "palette-source.png"),
                    "-vf", "palettegen=max_colors=256:stats_mode=full", str(palette)], check=True)
    return palette


def full_cuts(fast, length):
    """The whole recording, as stretches at normal speed and the sped-up ones between them."""
    cuts, t = [], 0.0
    for a, b, factor in fast:
        cuts += [(t, a, 1), (a, b, factor)]
        t = b
    return cuts + [(t, length + 1, 1)]


def make_animation(raw, captions, cuts, snaps, length, size, out, work, factor=1):
    """Writes `out`, a WebP or a GIF by its extension, from the stretches in `cuts` ((start, end, factor), in
    order). WebP is lossless, so the pictures keep every colour; a GIF has 256, shared with the text
    (findings/2026-11-13-demo-webp.md)."""
    width, height = size
    captions = [(t, text.replace("{factor}", str(factor))) for t, text in captions]
    pngs = render_captions(captions, width, work / out.name)
    inputs, chain = [], [f"[0]pad=iw:ih+{CAPTION_H}:0:0:color=0x11111b[v0]"]
    for i, (png, (start, _)) in enumerate(zip(pngs, captions)):
        end = captions[i + 1][0] if i + 1 < len(captions) else length + 1
        inputs += ["-i", str(png)]
        chain.append(f"[v{i}][{i + 1}]overlay=0:{height}:enable='between(t,{start:.2f},{end:.2f})'[v{i + 1}]")
    last = f"v{len(pngs)}"
    if cuts != [(0.0, length + 1, 1)]:
        # Cut the stretches out, each at its speed, then join them. Captions are already on, at the times they
        # were recorded with.
        chain.append(f"[{last}]split={len(cuts)}" + "".join(f"[s{i}]" for i in range(len(cuts))))
        for i, (a, b, factor) in enumerate(cuts):
            chain.append(f"[s{i}]trim=start={a:.3f}:end={b:.3f},setpts=(PTS-STARTPTS)/{factor}[g{i}]")
        # Back to FPS frames a second: browsers show GIF frames shorter than 2/100 s as 1/10 s.
        chain.append("".join(f"[g{i}]" for i in range(len(cuts))) + f"concat=n={len(cuts)}:v=1:a=0,fps={FPS}[sped]")
        last = "sped"
    cmd = ["ffmpeg", "-v", "error", "-y", "-i", str(raw), *inputs]
    if out.suffix == ".webp":
        # Lossless, at a steady FPS: libwebp merges repeated frames into one that lasts longer by itself.
        chain.append(f"[{last}]fps={FPS}")
        cmd += ["-filter_complex", ";".join(chain),
                "-c:v", "libwebp_anim", "-lossless", "1", "-compression_level", "6", "-loop", "0", str(out)]
    else:
        palette = make_palette(raw, snaps, pngs, work)
        chain.append(
            # Repeated frames are dropped, but one a second is kept: the GIF's last frame would otherwise
            # flash by, as its delay comes from the next frame's time.
            f"[{last}]mpdecimate=hi=512:lo=128:frac=0.1:max={FPS}[m];"
            f"[m][{len(pngs) + 1}]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle"
        )
        cmd += ["-i", str(palette), "-filter_complex", ";".join(chain), "-fps_mode", "vfr", "-loop", "0", str(out)]
    subprocess.run(cmd, check=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--tos", default=str(REPO / "target/release/tos"))
    ap.add_argument("--out", action="append",
                    help="an animation to write, .webp (lossless) or .gif; give it again for more "
                         "(default: docs/images/finder-demo.webp and .gif)")
    ap.add_argument("--short-out", action="append",
                    help="a short animation of the highlights (about 30 s) to write, .gif or .webp; give it again for "
                         "more (default, when --out isn't given either: docs/images/demo-short.gif)")
    ap.add_argument("--video", help="a video to show instead of the made-up one")
    ap.add_argument("--image", help="a picture, or a video's frame from 10%% in, to show instead of the made-up one")
    ap.add_argument("--no-claude", action="store_true", help="stop after the finder, without Claude Code")
    ap.add_argument("--terminal", choices=["ghostty", "xterm"], default="ghostty",
                    help="ghostty (kitty images, full colour; the default) or xterm (sixel images)")
    ap.add_argument("--keep", action="store_true", help="keep the work folder (raw video, captions)")
    args = ap.parse_args()
    outs = [Path(o) for o in args.out or [REPO / "docs/images/finder-demo.webp", REPO / "docs/images/finder-demo.gif"]]
    short_outs = [Path(o) for o in args.short_out or ([] if args.out else [REPO / "docs/images/demo-short.gif"])]
    for out in outs + short_outs:
        if out.suffix not in (".webp", ".gif"):
            sys.exit(f"{out}: the name must end in .webp or .gif")
    extra = []
    for flag, given in (("--video", args.video), ("--image", args.image)):
        if given:
            if not Path(given).expanduser().is_file():
                sys.exit(f"{given}: no such file")
            extra += [flag, str(Path(given).expanduser().resolve())]
    tos = Path(args.tos).resolve()
    if not tos.is_file():
        sys.exit(f"{tos} not found; run cargo build --release first")
    needs = ["Xvfb", "xdotool", "ffmpeg", "soffice", args.terminal]
    needs += ["dbus-run-session"] if args.terminal == "ghostty" else []
    needs += [] if args.no_claude else ["claude"]
    for prog in needs:
        if not shutil.which(prog):
            sys.exit(f"{prog} is needed")

    # Sockets must stay under about 100 bytes, so the folder is short and in /tmp. mkdtemp makes it private.
    work = Path(tempfile.mkdtemp(prefix="tosdemo.", dir="/tmp"))
    home = work / "home"
    folder = home / "bakery"
    display = free_display()
    env = {
        "HOME": str(home),
        "PATH": f"{tos.parent}:{os.environ.get('PATH', '/usr/bin:/bin')}",
        "DISPLAY": display,
        "LANG": "C.UTF-8",
        "SHELL": "/bin/bash",
        "TOS_SOCKET_DIR": str(work / "sock"),
        "TOS_STATE_DIR": str(work / "state"),
        "TOS_NO_UPDATE_CHECK": "1",
        # GTK (Ghostty) without gvfs, which would mount a FUSE folder in HOME, or accessibility services.
        "GIO_USE_VFS": "local",
        "GTK_A11Y": "none",
        "NO_AT_BRIDGE": "1",
    }
    # Every process started for the demo inherits this, which is how kill_leftovers finds them.
    marker = f"TOS_SOCKET_DIR={env['TOS_SOCKET_DIR']}".encode()
    procs = []
    try:
        subprocess.run([sys.executable, str(HERE / "make_files.py"), str(folder), *extra], check=True,
                       stdout=subprocess.DEVNULL)
        (home / ".bashrc").write_text('PS1="\\[\\e[1;32m\\]~/bakery\\[\\e[0m\\] $ "\n')
        (home / ".sudo_as_admin_successful").touch()  # Ubuntu's bash.bashrc prints a sudo hint without it
        if not args.no_claude:
            env["CLAUDE_CONFIG_DIR"] = str(claude_config(work, folder))
            env["DISABLE_AUTOUPDATER"] = "1"

        procs.append(subprocess.Popen(["Xvfb", display, "-screen", "0", "1280x800x24", "-nolisten", "tcp"],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        wait_for(lambda: Path(f"/tmp/.X11-unix/X{display[1:]}").exists(), "Xvfb")
        # No command: the terminal runs bash, and the tour types `tos attach` into it.
        procs.append(subprocess.Popen(terminal_command(args.terminal, folder), env=env, cwd=folder,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        # Ghostty also has an unmapped 1×1 window of the same class.
        window_class = "com.mitchellh.ghostty" if args.terminal == "ghostty" else "xterm"
        window = subprocess.run(["xdotool", "search", "--sync", "--onlyvisible", "--class", window_class], env=env,
                                check=True, capture_output=True, text=True).stdout.split()[0]
        info = subprocess.run(["xwininfo", "-id", window], env=env, capture_output=True, text=True).stdout
        size = tuple(int(line.split(":")[1]) for line in info.splitlines()
                     if line.strip().startswith(("Width:", "Height:")))
        subprocess.run(["xdotool", "windowfocus", "--sync", window], env=env, check=True)
        time.sleep(2.5)  # bash's prompt shows

        raw = work / "raw.mkv"
        t0 = time.monotonic()  # about when the first frame is taken; captions are timed from here
        rec = subprocess.Popen(
            ["ffmpeg", "-v", "error", "-y", "-f", "x11grab", "-draw_mouse", "0", "-framerate", str(FPS),
             "-video_size", f"{size[0]}x{size[1]}", "-i", f"{display}+0,0",
             "-c:v", "libx264rgb", "-preset", "ultrafast", "-crf", "0", str(raw)],
            stdin=subprocess.PIPE, env=env)
        time.sleep(0.5)
        demo = Demo(env, t0, tos)
        try:
            start(demo)
            if not args.no_claude:
                claude(demo, folder)
            tour(demo, server=not args.no_claude)
        finally:
            length = time.monotonic() - demo.t0
            rec.communicate(b"q", timeout=30)

        jobs = [(out, full_cuts(demo.fast, length), demo.factors["full"]) for out in outs]
        jobs += [(out, demo.short, demo.factors["short"]) for out in short_outs]
        for out, cuts, factor in jobs:
            out.parent.mkdir(parents=True, exist_ok=True)
            make_animation(raw, demo.captions, cuts, demo.snaps, length, size, out, work, factor)
            shown = sum((min(b, length) - a) / f for a, b, f in cuts)
            print(f"{out}: {out.stat().st_size / 1e6:.1f} MB, {shown:.0f} s ({length:.0f} s recorded), "
                  f"{size[0]}x{size[1] + CAPTION_H}")
        for a, b, f in demo.fast:
            print(f"  sped up {f}x: {a:.1f}-{b:.1f} s")
    finally:
        subprocess.run([str(tos), "kill-all-sessions", "-y"], env=env,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for p in reversed(procs):
            p.terminate()
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
        kill_leftovers(work, marker)
        if args.keep:
            # The copy of your Claude login isn't kept.
            shutil.rmtree(work / "claude", ignore_errors=True)
            print(f"kept {work}")
        else:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
