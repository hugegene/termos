# Finding: recording the demo GIF in Ghostty (2026-11-13)

## Question

The user asked to record the demo in Ghostty instead of xterm. Three questions decided how:
- Does Ghostty run on the virtual display (Xvfb)?
- Does it leave anything behind?
- Ghostty draws pictures in full colour (the kitty protocol), unlike xterm's sixel with 216 colours. How should
  the GIF's 256 colours be chosen so the pictures don't band and the text stays clean?

## Method

Dev machine (Ubuntu 24.04, Linux 6.8). Software:
- Ghostty `1.3.1~ppa2-noble1` (`/usr/bin/ghostty`, from a PPA)
- GTK 4.14.5, Mesa 25.2.8
- Xvfb, ffmpeg 6.1.1
- `tos` built from `c5a1a39`

**By hand,** on Xvfb `:89`:

```
ghostty --config-default-files=false --gtk-single-instance=false --window-decoration=false
        --theme="Catppuccin Mocha" --font-size=10 --window-width=104 --window-height=34
        --cursor-style-blink=false --resize-overlay=never --confirm-close-surface=false
        --shell-integration=none --working-directory=<folder>
```

- The environment was built from scratch (`env -i`), with no D-Bus session.
- Checks:
  - the window, with `xwininfo`
  - Ghostty's log
  - `tos`'s log (which image method it chose)
  - processes whose environment held the demo's `TOS_SOCKET_DIR`
  - `/proc/mounts`

**The script:** `python3 scripts/demo/record.py --out <gif> --keep`, now with Ghostty run under
`dbus-run-session`, and with `GIO_USE_VFS=local`, `GTK_A11Y=none` and `NO_AT_BRIDGE=1`. The same checks were made
afterwards.

**GIF palettes:** these were encoded from one run's raw video (104 s, without captions), with
`mpdecimate=hi=512:lo=128:frac=0.1:max=15` and `paletteuse=…:diff_mode=rectangle`. A frame of `sunset.png`, of
the PDF page and of code were cropped and looked at by eye.

## Results

**Ghostty on Xvfb:**

```
info(opengl): loaded OpenGL 4.5
window: "Ghostty" ("ghostty" "com.mitchellh.ghostty") 836x582+0+0, plus an unmapped 1x1 window of the same class
tos: client 2 shows images with Graphics { protocol: Kitty, cell_px: Some((8, 17)) }
```

- **Window size between runs:** 836×582 in two runs and 832×578 in one. `tos` reported `attached (104x34)` in
  both kinds of run.
- **`xdotool search --class`:** without `--onlyvisible`, it can pick the 1×1 window. `windowfocus` then fails
  with `BadMatch`.

**Left behind without a D-Bus session.** These processes carried the demo's environment:

```
dbus-launch --autolaunch=… ; dbus-daemon --session
xdg-desktop-portal, xdg-document-portal, xdg-permission-store, xdg-desktop-portal-gtk
gvfsd, gvfsd-fuse <demo HOME>/.gvfs -f        (a FUSE mount inside the folder to be deleted)
at-spi-bus-launcher, dbus-daemon (accessibility), at-spi2-registryd
```

**With `dbus-run-session` and the variables above** (three runs of the script): after clean-up, no process
carried the demo's environment, and nothing was mounted under the folder.

**GIF palettes**, for the 104 s raw video:

| | Palette | Dither | Bytes | Sunset | Background and text |
|---|---|---|---|---|---|
| A | every frame (`stats_mode=full`) | bayer, scale 5 | 832 387 | about 15 visible bands | solid |
| B | every frame | bayer, scale 2 | 1 032 807 | smooth | the dark background speckled |
| C | one per frame (`stats_mode=single`, `new=1`) | bayer, scale 5 | 10 319 270 | smooth | solid |
| D | every frame | `sierra2_4a` | 899 285 | blotchy, noisy | solid |
| E5 | 6 frames: text, video, PDF (as text), sunset, Claude, code | bayer, scale 5 | 826 032 | fine bands | solid |
| E4 | the same | bayer, scale 4 | 878 185 | faint bands | solid |

**The next run banded again** with E4's settings. Its PDF frame had caught the page as an image, white with grey
text, which takes up palette entries. These tests used that run's frames and the 24 caption images, on the sunset
and the PDF page:

| Frames in the palette | Sunset | PDF page |
|---|---|---|
| all six once | visible bands | white |
| without the PDF | smooth | light grey, with bluish table lines |
| sunset twice | nearly smooth | not checked |
| sunset twice, with the PDF | slight bands | not checked |
| **sunset 3×, video 2×, the rest once** | smooth | white |

**The final run** used that last palette and bayer scale 4:

```
GIF: 971 344 bytes, 93.27 s, 296 frames, 836x622; Claude 76.1–88.2 s shown at 2x
```

## Conclusions

- **Ghostty works headless.** It needs software OpenGL (Mesa) and these settings:
  - `--gtk-single-instance=false`, so it can't hand the window to a Ghostty the user has open
  - `--config-default-files=false`
  - `--onlyvisible` when searching for its window
- **It needs its own D-Bus** (`dbus-run-session`), and gvfs and accessibility off. Otherwise portals, gvfs and a
  FUSE mount outlive it.
  - The script finds every process it started by the demo's `TOS_SOCKET_DIR` in their environment, ends them,
    and unmounts anything under its folder before deleting it.
- **Full-colour pictures need a palette chosen for them.** Out of all the frames, screens of text crowd the
  pictures out of the 256 colours.
  - **Harder dithering:** speckles the background.
  - **A palette per frame:** costs 10× the size.
  - **Instead:** a palette from a few marked frames and the captions, with gradients counted more than once,
    and light dithering.
- **The window may be 832 or 836 pixels wide.** Both hold 104 columns, and both are about the README's width.
- **Unknown:** other Ghostty versions, and Ghostty on Wayland-only machines. The script needs Xvfb, so X11.
