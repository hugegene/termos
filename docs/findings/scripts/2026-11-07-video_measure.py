#!/usr/bin/env python3
"""Times ffprobe and ffmpeg for the finder's video preview: reading a video's facts, and grabbing one frame.

See ../2026-11-07-video-preview-ffmpeg.md.

Usage:
  2026-11-07-video_measure.py corpus DIR          make the synthetic test videos in DIR (needs ffmpeg with x264, x265)
  2026-11-07-video_measure.py run FILE...         time ffprobe and the frame grabs on each file
  2026-11-07-video_measure.py run --run2 FILE...  only the second run's cases (one thread, keyframes only)
  2026-11-07-video_measure.py run --run3 FILE...  only the third run's cases (default threads, full size)
"""
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

RUNS = 9
WIDTH = 1000  # the pane width in pixels the PDF findings used


def corpus(d: Path) -> None:
    d.mkdir(parents=True, exist_ok=True)
    # A film-length file: 60 minutes of 640×360 H.264 in Matroska, a keyframe every 10 s.
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=24",
                    "-t", "3600", "-c:v", "libx264", "-preset", "ultrafast", "-g", "240",
                    str(d / "long-h264.mkv")], check=True)
    # A 4K phone-style file: 10 s of 3840×2160 HEVC, a keyframe every 2 s.
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=3840x2160:rate=30",
                    "-t", "10", "-c:v", "libx265", "-preset", "ultrafast", "-x265-params", "keyint=60:log-level=0",
                    "-tag:v", "hvc1", str(d / "uhd-hevc.mp4")], check=True)


def timed(args: list[str]) -> tuple[float, int, str]:
    """Median wall time in ms over RUNS, the size of stdout, and the first line of stderr."""
    times, size, err = [], 0, ""
    for _ in range(RUNS):
        s = time.perf_counter()
        r = subprocess.run(args, capture_output=True)
        times.append((time.perf_counter() - s) * 1000)
        size, err = len(r.stdout), (r.stderr.decode(errors="replace").splitlines() or [""])[0]
    return statistics.median(times), size, err


def run(files: list[str]) -> None:
    for f in files:
        probe = ["ffprobe", "-v", "error", "-select_streams", "v:0",
                 "-show_entries", "format=duration:stream=codec_name,width,height,r_frame_rate:stream_tags=rotate"
                 ":stream_side_data=rotation", "-of", "json", f]
        facts = json.loads(subprocess.run(probe, capture_output=True, check=True).stdout)
        st, dur = facts["streams"][0], float(facts["format"]["duration"])
        size = Path(f).stat().st_size
        print(f"\n## {Path(f).name}: {st['codec_name']} {st['width']}×{st['height']}, {dur:.1f} s, "
              f"{size / 1e6:.1f} MB")
        ms, _, _ = timed(probe)
        print(f"| ffprobe (json) | {ms:.1f} ms | |")
        at = f"{dur * 0.1:.3f}"
        # Run 1 scaled every frame to WIDTH (enlarging small ones). Run 2 only shrinks, to the image preview's
        # 1600-pixel longest side.
        scale = ["-vf", f"scale={WIDTH}:-2"]
        cap = ["-vf", "scale=w='min(1600,iw)':h='min(1600,ih)':force_original_aspect_ratio=decrease"]
        out = ["-frames:v", "1", "-f", "image2pipe", "-c:v", "ppm", "-"]
        one = ["-threads", "1"]
        cases = {
            # run 1
            "frame at 0, scaled": ["-i", f, *scale, *out],
            "frame at 10%, accurate seek, scaled": ["-ss", at, "-i", f, *scale, *out],
            "frame at 10%, keyframe only (-noaccurate_seek), scaled": ["-ss", at, "-noaccurate_seek", "-i", f,
                                                                      *scale, *out],
            "frame at 10%, accurate seek, full size": ["-ss", at, "-i", f, *out],
            # run 2: one decoder thread, and decoding keyframes only
            "frame at 0, -threads 1, full size": [*one, "-i", f, *out],
            "frame at 10%, accurate seek, -threads 1, full size": ["-ss", at, *one, "-i", f, *out],
            "frame at 10%, keyframes only (-skip_frame nokey), -threads 1, full size":
                ["-ss", at, "-noaccurate_seek", "-skip_frame", "nokey", *one, "-i", f, *out],
            "frame at 10%, keyframes only, -threads 1, shrunk to 1600": ["-ss", at, "-noaccurate_seek", "-skip_frame",
                                                                         "nokey", *one, "-i", f, *cap, *out],
            # run 3: the default thread count again, without scaling, for a like-for-like comparison with run 2
            "frame at 0, default threads, full size": ["-i", f, *out],
            "frame at 10%, keyframes only, default threads, full size": ["-ss", at, "-noaccurate_seek", "-skip_frame",
                                                                        "nokey", "-i", f, *out],
        }
        if "--run2" in sys.argv:
            cases = dict(list(cases.items())[4:8])
        if "--run3" in sys.argv:
            cases = dict(list(cases.items())[8:])
        for name, args in cases.items():
            ms, n, err = timed(["ffmpeg", "-v", "error", "-nostdin", *args])
            print(f"| {name} | {ms:.1f} ms | {n:,} bytes | {err}")


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "corpus":
        corpus(Path(sys.argv[2]))
    elif len(sys.argv) >= 3 and sys.argv[1] == "run":
        run([a for a in sys.argv[2:] if not a.startswith("--run")])
    else:
        sys.exit(__doc__)
