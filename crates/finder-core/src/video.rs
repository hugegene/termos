//! Video previews through ffmpeg's command-line tools (`ffprobe` and `ffmpeg`, package ffmpeg), run as separate
//! programs, as PDF previews run poppler. See `docs/plans/2026-11-07-video-and-office-previews.md`, and
//! `docs/findings/2026-11-07-video-preview-ffmpeg.md` for the timings behind the choices here.
//!
//! Paths are passed to ffmpeg as they are, so they should be absolute: a relative name with a colon in it, such
//! as `a:b.mp4`, would be read as a protocol.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use image::RgbImage;

use crate::Limits;
use crate::tool::{self, ToolError};

/// What `ffprobe` says about a video's first video stream.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    /// Length in seconds, if the file says (some broken or streamed files don't).
    pub duration: Option<f64>,
    /// The size as it's shown, after the rotation phones record: a portrait clip is narrower than it is tall.
    pub width: u32,
    pub height: u32,
    /// For example "h264".
    pub codec: Option<String>,
}

impl VideoInfo {
    /// The header's facts, for example "1:23 · 1920×1080 · h264".
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(d) = self.duration {
            parts.push(clock(d));
        }
        parts.push(format!("{}×{}", self.width, self.height));
        if let Some(c) = &self.codec {
            parts.push(c.clone());
        }
        parts.join(" · ")
    }
}

/// A length in seconds as "0:02", "1:23" or "1:02:03".
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// The file extensions shown as videos. `.ts` is left out: it's TypeScript far more often than MPEG transport
/// stream.
pub const EXTENSIONS: &[&str] =
    &["mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "mpg", "mpeg", "m2ts", "3gp", "ogv"];

/// Time limit for each program. ffprobe took 30–110 ms and a frame 34–232 ms in the finding.
const TOOL_TIMEOUT: Duration = Duration::from_secs(20);

/// Frames bigger than 4K (3840×2160 pixels) are shrunk by ffmpeg, which keeps the pipe under about 25 MB.
const BIG_FRAME: u64 = 3840 * 2160;

/// How to install ffmpeg, for the message when it's missing.
#[cfg(target_os = "macos")]
const INSTALL_FFMPEG: &str = "brew install ffmpeg";
#[cfg(not(target_os = "macos"))]
const INSTALL_FFMPEG: &str = "package ffmpeg";

fn missing() -> String {
    format!("video preview needs ffmpeg ({INSTALL_FFMPEG})")
}

/// ffmpeg starts its messages with the part that wrote them: "[mov,mp4,m4a,3gp,3g2,mj2 @ 0x5bb4…] moov atom
/// not found". Only the message is kept.
fn explain(e: ToolError) -> ToolError {
    match e {
        ToolError::Failed(msg) if msg.starts_with('[') && msg.contains(" @ ") => match msg.split_once("] ") {
            Some((_, rest)) => ToolError::Failed(rest.to_owned()),
            None => ToolError::Failed(msg),
        },
        other => other,
    }
}

/// The duration, the shown size and the codec of `path`'s first video stream.
pub fn info(path: &Path, cancel: &dyn Fn() -> bool) -> Result<VideoInfo, ToolError> {
    info_with("ffprobe", path, cancel)
}

fn info_with(program: &str, path: &Path, cancel: &dyn Fn() -> bool) -> Result<VideoInfo, ToolError> {
    let mut cmd = Command::new(program);
    cmd.args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .arg("format=duration:stream=codec_name,width,height:stream_tags=rotate:stream_side_data=rotation")
        .args(["-of", "json"])
        .arg(path);
    let out = tool::run(cmd, 1 << 20, Some(TOOL_TIMEOUT), cancel, &missing()).map_err(explain)?;
    parse_info(&out.stdout)
}

fn parse_info(json: &[u8]) -> Result<VideoInfo, ToolError> {
    let v: serde_json::Value =
        serde_json::from_slice(json).map_err(|e| ToolError::Failed(format!("can't read what ffprobe said: {e}")))?;
    let Some(stream) = v["streams"].get(0) else {
        return Err(ToolError::Failed("no video in this file".into()));
    };
    let size = |key: &str| stream[key].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
    let (mut width, mut height) = (size("width"), size("height"));
    // Newer files carry a display matrix ("rotation": -90); older ones a "rotate" tag ("90").
    let rotation = stream["side_data_list"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|d| d["rotation"].as_f64())
        .or_else(|| stream["tags"]["rotate"].as_str().and_then(|r| r.trim().parse().ok()))
        .unwrap_or(0.0);
    if (rotation.round() as i64).rem_euclid(180) == 90 {
        (width, height) = (height, width);
    }
    let duration = v["format"]["duration"].as_str().and_then(|d| d.parse::<f64>().ok()).filter(|d| d.is_finite());
    let codec = stream["codec_name"].as_str().map(str::to_owned);
    Ok(VideoInfo { duration, width, height, codec })
}

/// One frame from 10% of the way in (thumbnailers do the same, to skip black or faded-in openings), shrunk to
/// `limits.max_image_side`. Falls back to the first frame when there's none there.
pub fn frame(path: &Path, info: &VideoInfo, limits: &Limits, cancel: &dyn Fn() -> bool) -> Result<RgbImage, ToolError> {
    frame_with("ffmpeg", path, info, limits, cancel)
}

fn frame_with(
    program: &str,
    path: &Path,
    info: &VideoInfo,
    limits: &Limits,
    cancel: &dyn Fn() -> bool,
) -> Result<RgbImage, ToolError> {
    let at = info.duration.map_or(0.0, |d| d * 0.1);
    let img = match grab(program, path, info, at, limits, cancel)? {
        Some(img) => img,
        // Seeking can land past the last keyframe of a short or oddly cut file.
        None if at > 0.0 => grab(program, path, info, 0.0, limits, cancel)?
            .ok_or_else(|| ToolError::Failed("no frame in this file".into()))?,
        None => return Err(ToolError::Failed("no frame in this file".into())),
    };
    let side = limits.max_image_side;
    if img.width() > side || img.height() > side {
        return Ok(image::DynamicImage::ImageRgb8(img).thumbnail(side, side).into_rgb8());
    }
    Ok(img)
}

/// The keyframe at or before `at` seconds, or `None` if ffmpeg wrote nothing.
fn grab(
    program: &str,
    path: &Path,
    info: &VideoInfo,
    at: f64,
    limits: &Limits,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<RgbImage>, ToolError> {
    let mut cmd = Command::new(program);
    cmd.args(["-v", "error", "-nostdin"]);
    if at > 0.0 {
        // Keyframes only: an accurate seek took up to 579 ms on 4K HEVC in the finding.
        cmd.args(["-ss", &format!("{at:.3}"), "-noaccurate_seek"]);
    }
    // One thread was 1.4–2.8× faster for a single frame.
    cmd.args(["-skip_frame", "nokey", "-threads", "1", "-i"]).arg(path);
    if u64::from(info.width) * u64::from(info.height) > BIG_FRAME {
        let side = limits.max_image_side;
        cmd.args(["-vf", &format!("scale=w={side}:h={side}:force_original_aspect_ratio=decrease")]);
    }
    // Raw PPM on stdout, as for PDF pages.
    cmd.args(["-frames:v", "1", "-f", "image2pipe", "-c:v", "ppm", "-"]);
    // A 4K frame is 24.9 MB of PPM; anything bigger was shrunk, unless ffprobe got the size wrong.
    let max_out = (BIG_FRAME * 3 + 4096) as usize;
    let out = tool::run(cmd, max_out, Some(TOOL_TIMEOUT), cancel, &missing()).map_err(explain)?;
    if out.truncated {
        return Err(ToolError::Failed("frame too large to show".into()));
    }
    if out.stdout.is_empty() {
        return Ok(None);
    }
    let img = image::load_from_memory_with_format(&out.stdout, image::ImageFormat::Pnm)
        .map_err(|e| ToolError::Failed(format!("can't read the frame: {e}")))?;
    Ok(Some(img.into_rgb8()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Instant;

    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
    }

    /// Tests that run ffmpeg skip, saying so, when it isn't installed.
    fn have_ffmpeg() -> bool {
        let ok = ["ffprobe", "ffmpeg"].iter().all(|t| Command::new(t).arg("-version").output().is_ok());
        if !ok {
            eprintln!("skipped: ffmpeg isn't installed");
        }
        ok
    }

    const NEVER: &dyn Fn() -> bool = &|| false;

    #[test]
    fn clock_times() {
        assert_eq!(clock(2.0), "0:02");
        assert_eq!(clock(83.4), "1:23");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(-1.0), "0:00");
    }

    #[test]
    fn parses_ffprobe() {
        let json = br#"{"streams":[{"codec_name":"h264","width":1920,"height":1080,"tags":{}}],
                        "format":{"duration":"83.400000"}}"#;
        let info = parse_info(json).unwrap();
        assert_eq!(info, VideoInfo { duration: Some(83.4), width: 1920, height: 1080, codec: Some("h264".into()) });
        assert_eq!(info.summary(), "1:23 · 1920×1080 · h264");

        // Turned by a display matrix, or by an old "rotate" tag.
        let json = br#"{"streams":[{"width":1920,"height":1080,"side_data_list":[{"rotation":-90}]}],"format":{}}"#;
        let info = parse_info(json).unwrap();
        assert_eq!((info.width, info.height, info.duration), (1080, 1920, None));
        assert_eq!(info.summary(), "1080×1920");
        let json = br#"{"streams":[{"width":640,"height":480,"tags":{"rotate":"270"}}],"format":{"duration":"N/A"}}"#;
        assert_eq!(parse_info(json).unwrap().width, 480);
        let json = br#"{"streams":[{"width":640,"height":480,"tags":{"rotate":"180"}}],"format":{}}"#;
        assert_eq!(parse_info(json).unwrap().width, 640);

        match parse_info(br#"{"streams":[],"format":{"duration":"3.0"}}"#) {
            Err(ToolError::Failed(msg)) => assert_eq!(msg, "no video in this file"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ffmpeg_messages_lose_their_prefix() {
        let e = explain(ToolError::Failed("[mov,mp4,m4a,3gp,3g2,mj2 @ 0x5bb4] moov atom not found".into()));
        assert_eq!(e.to_string(), "moov atom not found");
        assert_eq!(explain(ToolError::Failed("[plain] text".into())).to_string(), "[plain] text");
    }

    #[test]
    fn reads_a_video_and_its_frame() {
        if !have_ffmpeg() {
            return;
        }
        let path = data("two-seconds.mp4");
        let info = info(&path, NEVER).unwrap();
        assert_eq!(info.summary(), "0:02 · 64×48 · h264");
        let img = frame(&path, &info, &Limits::default(), NEVER).unwrap();
        assert_eq!((img.width(), img.height()), (64, 48));
        // ffmpeg's testsrc pattern: red near the left of the top row, white at its right.
        let red = img.get_pixel(12, 2);
        assert!(red[0] > 200 && red[1] < 60 && red[2] < 60, "{red:?}");
        let white = img.get_pixel(60, 2);
        assert!(white.0.iter().all(|c| *c > 200), "{white:?}");

        // A smaller limit shrinks the frame, keeping its shape.
        let limits = Limits { max_image_side: 32, ..Limits::default() };
        let img = frame(&path, &info, &limits, NEVER).unwrap();
        assert_eq!((img.width(), img.height()), (32, 24));
    }

    #[test]
    fn rotated_videos_are_shown_upright() {
        if !have_ffmpeg() {
            return;
        }
        let path = data("rotated.mp4");
        let info = info(&path, NEVER).unwrap();
        assert_eq!((info.width, info.height), (48, 64));
        let img = frame(&path, &info, &Limits::default(), NEVER).unwrap();
        assert_eq!((img.width(), img.height()), (48, 64));
    }

    #[test]
    fn past_the_last_keyframe_falls_back_to_the_first() {
        if !have_ffmpeg() {
            return;
        }
        // A duration that puts 10% past the end, as a wrong header might.
        let path = data("two-seconds.mp4");
        let info = VideoInfo { duration: Some(60.0), width: 64, height: 48, codec: None };
        let img = frame(&path, &info, &Limits::default(), NEVER).unwrap();
        assert_eq!(img.width(), 64);
    }

    #[test]
    fn broken_files_and_missing_ffmpeg() {
        if have_ffmpeg() {
            match info(&data("truncated.mp4"), NEVER) {
                Err(ToolError::Failed(msg)) => assert_eq!(msg, "moov atom not found"),
                other => panic!("{other:?}"),
            }
        }
        let path = data("two-seconds.mp4");
        match info_with("tos-no-such-ffprobe", &path, NEVER) {
            Err(ToolError::Missing(msg)) => assert!(msg.starts_with("video preview needs ffmpeg ("), "{msg}"),
            other => panic!("{other:?}"),
        }
        let info = VideoInfo { duration: Some(2.0), width: 64, height: 48, codec: None };
        match frame_with("tos-no-such-ffmpeg", &path, &info, &Limits::default(), NEVER) {
            Err(ToolError::Missing(msg)) => assert!(msg.starts_with("video preview needs ffmpeg"), "{msg}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_newer_request_stops_it() {
        // A fake ffmpeg that never finishes stands in for a slow file.
        // It's written by another program: a file this process had open for writing can be "busy" when run,
        // if a test on another thread forked meanwhile.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("ffmpeg");
        let script = r#"printf '#!/bin/sh\nexec sleep 5\n' > "$1" && chmod +x "$1""#;
        assert!(Command::new("sh").args(["-c", script, "sh"]).arg(&fake).status().unwrap().success());
        let flag = AtomicBool::new(false);
        let info = VideoInfo { duration: Some(2.0), width: 64, height: 48, codec: None };
        let start = Instant::now();
        thread::scope(|s| {
            s.spawn(|| {
                thread::sleep(Duration::from_millis(30));
                flag.store(true, Ordering::Relaxed);
            });
            let cancel = || flag.load(Ordering::Relaxed);
            let res = frame_with(fake.to_str().unwrap(), &data("two-seconds.mp4"), &info, &Limits::default(), &cancel);
            assert!(matches!(res, Err(ToolError::Cancelled)), "{res:?}");
        });
        assert!(start.elapsed() < Duration::from_secs(1), "{:?}", start.elapsed());
    }
}
