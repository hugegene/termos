# Finding: the demo with the user's Ghibli video and picture (2026-11-13)

## Question

The user found the made-up sunset and Mandelbrot video plain, and asked for a video and a picture from
`~/ghibli-video`, and for both a GIF and a WebP. Three things decided how:
- which files to use
- how the finder shows them
- what both formats cost from one recording

## Method

`~/ghibli-video` is the user's ComfyUI project. It turns a video into Studio Ghibli style (its `PLAN.md`).

- **Listing:** its videos and pictures, with `ffprobe` (length, size, codec) and `identify`.
- **Looking:** each video's frame from 10% in (what the finder shows) and each picture, by eye.
- **The recording:**

  ```
  python3 scripts/demo/record.py --video ~/ghibli-video/output/full_v5_depth8/babyplayful_ghibli.mp4 \
      --image ~/ghibli-video/output/t2v/flying_baby.mp4 --out <dir>/demo.webp --out <dir>/demo.gif
  ```

  Ghostty 1.3.1 and `tos` from `c5a1a39`, the rest as in [the WebP finding](2026-11-13-demo-webp.md).
- **Checks:**
  - frame times: the WebP's from its `ANMF` chunks, the GIF's from Pillow
  - the frames at 26.5 s (the video), 65 s (the picture), and the end

## Results

**Candidates:**

| File | Size | What it shows (by eye) |
|---|---|---|
| `output/full_v5_depth8/babyplayful_ghibli.mp4` | 8.75 s, 480×832, h264 | Ghibli-style: a child seen from behind, at a sofa |
| `output/full_v4/babyplayful_ghibli.mp4` | 480×832 | the same scene, an earlier version |
| `output/t2v/flying_baby.mp4` | 5.06 s, 832×480, h264 | made from text only: a baby in a red cape flying among clouds |
| `reference/baby_walker_480x832.png`, `input/baby_walker.png` | 480×832, 447×447 | a photo of a branded toy baby walker, like a shop's product photo |
| `input/babyplayful.mp4` | 7.5 s, 1080×1920 | the original footage of a real child |

**The finder's previews:**
- `movie.mp4` reads `video · 0:09 · 480×832 · h264 · 2.9 MB`, with the frame of the child at the sofa.
- `snapshot.png` (the cape frame, 10% into `flying_baby.mp4`) reads `image · 832×480 · 406.8 KB`.

**Both formats from one recording:**

```
recorded 98 s; Claude 76.1–86.7 s shown at 2x
finder-demo.webp  1 438 286 bytes, 260 frames, 93.132 s, 832x618
finder-demo.gif   1 113 586 bytes, 295 frames, 92.47 s, 832x618
```

**By eye:** the WebP matches the screen. The GIF is close, but slightly posterised on skin and in the sky.

## Conclusions

- **Used:**
  - **The video step:** the latest Ghibli-style video (`full_v5_depth8`). It's stylised and seen from behind.
  - **The picture:** a frame of `flying_baby.mp4`. It's generated from text, so no real person or third-party photo
    is in it.
- **Not used:**
  - **The baby-walker PNGs:** they look like a third party's product photo, which may not be free to publish. They
    can be swapped in with `--image` if they're the user's to publish.
  - **The original footage:** it's the real child.
- **Both formats cost one recording:** encoding twice from one raw video is cheap.
  - The WebP is 29% bigger than the GIF, and better.
  - The README shows the WebP; the GIF sits beside it.
- **Unknown:** whether GitHub plays the animated WebP in the README. It's still unchecked. If it doesn't, the
  README can point at the GIF instead.
