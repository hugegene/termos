# Finding: reading a video's facts and grabbing one frame with ffprobe and ffmpeg (2026-11-07)

## Question

The [video preview plan](../plans/2026-11-07-video-and-office-previews.md) shows a still frame, the duration and
the resolution, using ffprobe and ffmpeg as separate programs (as the PDF preview runs poppler). Three decisions
depend on how fast they are:

1. **Which frame:** the first one, or one 10% in (what thumbnailers use, to skip black or faded-in openings)?
2. **How ffmpeg should decode it:** its default threading or one thread; every frame, or keyframes only?
3. **Where to shrink it:** in ffmpeg's scaler, or afterwards in `tb`, as image previews already are?

Also: is `mpv` installed for Enter, and is there another player?

## Method

Dev machine: 12th Gen Intel Core i7-12650H (16 CPUs), 31 GiB RAM, Ubuntu, Linux 6.8.0-147, CPU governor
`powersave`. ffmpeg and ffprobe 6.1.1-3ubuntu5, Python 3.12.3.

The script is [scripts/2026-11-07-video_measure.py](scripts/2026-11-07-video_measure.py):

```
python3 -I docs/findings/scripts/2026-11-07-video_measure.py corpus DIR
python3 -I docs/findings/scripts/2026-11-07-video_measure.py run FILE...          # run 1
python3 -I docs/findings/scripts/2026-11-07-video_measure.py run --run2 FILE...   # run 2
python3 -I docs/findings/scripts/2026-11-07-video_measure.py run --run3 FILE...   # run 3
```

- **Each number** is the median wall time of 9 runs through `subprocess.run`, including starting the program.
- **ffprobe:** `ffprobe -v error -select_streams v:0 -show_entries format=duration:stream=codec_name,width,height,r_frame_rate:stream_tags=rotate:stream_side_data=rotation -of json FILE`
- **A frame:** `ffmpeg -v error -nostdin [-ss T] [options] -i FILE [-vf …] -frames:v 1 -f image2pipe -c:v ppm -`.
  The raw PPM on stdout is what `tb` would read.
  - `T` is 10% of the duration.
  - **"accurate seek"** is ffmpeg's default with `-ss` before `-i`. It decodes from the keyframe before `T` up to
    `T`.
  - **"keyframes only"** adds `-noaccurate_seek -skip_frame nokey`. It takes the keyframe at or before `T`, and
    decodes nothing else.
  - **"scaled"** (run 1) is `-vf scale=1000:-2`, which also enlarges small videos.
  - **"shrunk to 1600"** (run 2) only shrinks, to the image preview's 1600-pixel longest side.
- **Files:**

  | name | what | codec | size in pixels | length | file size |
  |---|---|---|---|---|---|
  | `long-h264.mkv` | synthetic film (`testsrc2`, keyframe every 10 s), made by `corpus` | H.264 | 640×360 | 60 min | 783.9 MB |
  | `uhd-hevc.mp4` | synthetic 4K (`testsrc2`, keyframe every 2 s), made by `corpus` | HEVC | 3840×2160 | 10.0 s | 16.5 MB |
  | phone A | an edited phone clip | H.264 | 1080×1920 | 12.0 s | 27.5 MB |
  | phone B | a phone clip | VP9 | 1080×1920 | 73.0 s | 25.3 MB |
  | phone C | a phone clip | H.264 | 1920×1080 | 16.5 s | 35.6 MB |
  | phone D | a phone clip | VP9 | 1080×1920 | 2.7 s | 0.6 MB |

  The phone clips are the user's own, from `~/Yguards`. They're named by letter here, and only their format is
  recorded.
- **Load average:** 3.4 before run 1, 3.2 before run 2 and 1.1 before run 3. Another session was building at
  the time, so runs 1 and 2 are on the high side.

## Results

**ffprobe** (median, ms): `long` 29.0–30.8 · `uhd` 33.8–34.2 · A 45.0–45.3 · B 37.7–38.7 · C 104.6–111.5 · D 36.0–36.5.

**One frame** (median, ms; bytes of PPM read):

| case | long | uhd | A | B | C | D | bytes |
|---|---|---|---|---|---|---|---|
| **run 1** | | | | | | | |
| frame at 0, scaled | 68.1 | 281.8 | 192.8 | 205.5 | 258.3 | 192.7 | 1.7 MB (long, uhd) / 5.3 MB |
| at 10%, accurate seek, scaled | 68.3 | 377.3 | 199.5 | 284.3 | 295.3 | 213.5 | same |
| at 10%, keyframe (`-noaccurate_seek`), scaled | 68.8 | 425.3 | 204.9 | 352.3 | 336.7 | 223.9 | same |
| at 10%, accurate seek, full size | 51.3 | 462.5 | 150.7 | 235.0 | 233.1 | 163.8 | 0.7 / 24.9 / 6.2 MB |
| **run 2: one decoder thread** | | | | | | | |
| frame at 0, `-threads 1`, full size | **35.3** | **152.3** | **71.5** | **56.9** | 130.6 | **52.2** | 0.7 / 24.9 / 6.2 MB |
| at 10%, accurate seek, `-threads 1`, full size | 36.2 | 578.6 | 129.5 | 217.3 | 307.4 | 87.6 | same |
| at 10%, keyframes only, `-threads 1`, full size | 34.3 | 232.0 | 96.1 | 228.3 | **107.4** | 88.5 | same |
| at 10%, keyframes only, `-threads 1`, shrunk to 1600 | 34.0 | 240.9 | 119.0 | 310.6 | 132.5 | 120.5 | 0.7 / 4.3 MB |
| **run 3: default threads, like for like with run 2** | | | | | | | |
| frame at 0, default threads, full size | 51.0 | 369.9 | 133.6 | 152.7 | 202.6 | 147.7 | 0.7 / 24.9 / 6.2 MB |
| at 10%, keyframes only, default threads, full size | 41.5 | 275.5 | 103.9 | 238.0 | 115.9 | 173.1 | same |

**Players:** `mpv` isn't installed (`mpv: command not found`). `ffplay` 6.1.1 is, from the ffmpeg package, and so
is `xdg-open`.

## Conclusions

- **One decoder thread is faster for a single frame.** The first frame took 35–152 ms with `-threads 1`, against
  51–370 ms with the default (1.4–2.8× slower). Keyframes-only at 10% was also faster with one thread, on every
  file.
  - The likely reason: ffmpeg's frame threading decodes several frames ahead before it returns the first.
  - One thread also leaves the other cores free while you scroll through a folder.
- **10% in, keyframes only, costs 34–232 ms; the first frame costs 35–152 ms.**
  - Accurate seeking to 10% is worst: up to 579 ms on the 4K HEVC file. The keyframes-only form decodes only
    keyframes, so it doesn't work through the frames between the keyframe and `T`.
  - The 10% frame also needs the duration first, so ffprobe (30–110 ms) runs before it, not alongside it.
    Totals: about 64–270 ms at 10%, against 35–152 ms for the first frame with ffprobe run alongside. That's an
    **estimate**, added up from the medians above.
- **Shrink in `tb`, not in ffmpeg.** On the files it shrank, ffmpeg's scaler added 9–82 ms (run 2, "shrunk to
  1600" against full size). Reading the full frame costs little: the 25 MB 4K frame came through the pipe within
  the times above.
- **mpv must be installed for Enter**, or `tb` falls back to `ffplay`, which is there.
- **Not measured:**
  - 8K and other very large videos (an 8K frame is about 100 MB of PPM)
  - videos with rotation metadata: whether ffprobe's width and height need swapping for the header. ffmpeg
    rotates frames itself by default.
  - files on slow or network disks
  - whether the first frame is black more often than the 10% one, in real files
