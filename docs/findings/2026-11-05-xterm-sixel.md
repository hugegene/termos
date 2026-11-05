# Finding: `tb`'s finder images in xterm (2026-11-05)

## Question

The user asked whether `tb`'s finder works in xterm. Does image detection pick the right method, and do sixel
images look right? Whether `tb` needs to change for xterm depends on it.

## Method

Dev machine (Ubuntu, Linux 6.8), `xterm` 390 (package `390-1ubuntu3`; `strings /usr/bin/xterm` mentions sixel),
`tb` release build of 2026-11-04 (commit `62dcc2f` plus uncommitted pane cell-size changes; image code
unchanged since `62dcc2f`).

- **Defaults** from `man xterm`: `decTerminalID`, `decGraphicsID`, `numColorRegisters`, `maxGraphicSize`,
  `disallowedWindowOps`.
- **Off-screen runs:** everything ran on a virtual display (`Xvfb :77 -screen 0 1400x900x24`). Each xterm was
  `-geometry 200x60`, with the default font (6×13-pixel cells).
- **The `tb` setup:** a throwaway session `xt` (`TB_SOCKET_DIR=/tmp/tbt-xt`) with a finder on a folder holding
  the user's 480×832 PNG frame. Each xterm ran `tb -s xt attach`. After 5–6 s the screen was captured with
  `import -display :77 -window root`, and the session log showed the method chosen.
- **The colour-register probe** (`regs.py`, in raw mode) sent DA1 (`CSI c`), then XTSMGRAPHICS:
  - read colour registers (`CSI ? 1 ; 1 S`)
  - read the maximum (`CSI ? 1 ; 4 S`)
  - set 256 (`CSI ? 1 ; 3 ; 256 S`)
- **The xterm settings tried:**
  - defaults
  - `-ti vt340`
  - `-xrm 'XTerm*decGraphicsID: vt340'`
  - `-ti vt340 -xrm 'XTerm*numColorRegisters: 256'`
  - `-ti vt340` with a helper (`ask256.py`) that sends `CSI ? 1 ; 3 ; 256 S`, swallows the answer and then runs
    `tb attach`

## Results

**`man xterm`:**
- `decTerminalID` defaults to 420, and `decGraphicsID` to 0.
- `numColorRegisters` is unset; it follows `decTerminalID`: 16 for 340, 1024 for "other".
- `maxGraphicSize` defaults to 1000x1000.
- `disallowedWindowOps` defaults to `1,2,3,4,5,6,7,8,9,11,13,14,18,19,20,21,GetSelection,SetSelection,SetWinLines,SetXprop`.

**Method chosen by `tb`** (session log):

```
default:                 Graphics { protocol: Blocks, cell_px: Some((6, 13)) }
-ti vt340:               Graphics { protocol: Sixel, cell_px: Some((6, 13)) }
decGraphicsID vt340:     Graphics { protocol: Sixel, cell_px: Some((6, 13)) }
vt340 + 256 registers:   Graphics { protocol: Sixel, cell_px: Some((6, 13)) }
vt340 + asked for 256:   Graphics { protocol: Sixel, cell_px: Some((6, 13)) }
```

**Probe answers:**

```
default:        DA1=\e[?64;1;2;6;9;15;16;17;18;21;22;28c     set256=\e[?1;3S        (3: failure)
vt340:          DA1=\e[?63;1;2;4;6;9;15;16;22;28c            set256=\e[?1;0;256S    (0: success)
decGraphicsID:  DA1=\e[?64;1;2;4;6;9;15;16;17;18;21;22;28c   set256=\e[?1;0;256S
vt340+256:      DA1=\e[?63;1;2;4;6;9;15;16;22;28c            set256=\e[?1;0;256S
```

The read queries (`?1;1S`, `?1;4S`) got no answer within 1 s in any setting.

**Screenshots:**
- **Defaults:** coloured blocks.
- **`-ti vt340` and `decGraphicsID vt340`:** the picture at full resolution and the right shape (portrait,
  centred), but the colours are wrong: washed-out pinks, yellows and whites.
- **`vt340 + 256 registers` and `vt340 + asked for 256`:** the picture with correct colours, sharp and the right
  shape.

## Conclusions

- **Detection is right.** xterm's defaults have sixel off, so `tb` shows blocks. With sixel on, `tb` picks
  sixel and gets the cell size from the terminal.
- **The wrong colours come from colour registers.** In VT340 mode xterm has 16 colour registers, and `tb`'s
  sixel encoder defines 216, so later definitions overwrite earlier ones. The last 16 defined are bright
  pinks, yellows and whites, which is what showed. With 256 registers the picture is correct.
- **A program can raise the limit:** `CSI ? 1 ; 3 ; 256 S` succeeds whenever sixel is on. A fix in `tb`: when
  the client picks sixel, it sends that request during detection and reads the answer. If the request fails,
  it falls back to blocks.
- **Workaround without a fix:** `xterm -ti vt340 -xrm 'XTerm*numColorRegisters: 256'` (tested).
- **Not tested:**
  - images larger than `maxGraphicSize` (1000×1000 by default), which xterm may cut off
  - copying to the clipboard: OSC 52 (`SetSelection`) is in xterm's default `disallowedWindowOps`, so `tb`'s
    copy probably doesn't reach the clipboard without changing that setting
