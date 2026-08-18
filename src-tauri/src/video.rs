//! Turning a song into a shareable video.
//!
//! Suno's shareable MP4 is cover art plus audio, and that is genuinely all this
//! is — but it is the format every platform accepts when you want to post a
//! song, so the absence of it is felt.
//!
//! # Why this one is optional
//!
//! Everything else in Aria runs with no dependency the user has to install.
//! Video does not: encoding H.264 means ffmpeg, and shipping a build of it is a
//! different project. So this feature *detects* ffmpeg and hides itself when it
//! isn't there, rather than making every user carry a dependency for a feature
//! most of them will never open. Nothing else in the app degrades without it.
//!
//! The cover is handed over as PNG rather than the SVG we keep on disk, because
//! ffmpeg can only read SVG when built against librsvg — which distro and
//! static builds routinely are not. Rasterising it ourselves keeps this working
//! on whatever ffmpeg the user happens to have.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Edge of the video, in pixels. Square, the way music platforms present audio.
const SIZE: u32 = 1280;

/// A still image costs x264 almost nothing per frame, so this is chosen for
/// player and platform compatibility rather than to save bytes.
const FPS: u32 = 30;

/// What the machine can currently do, so the UI can offer only what will work.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VideoSupport {
    pub available: bool,
    /// Shown when it isn't, so the answer isn't just a missing button.
    pub reason: Option<String>,
}

pub fn support() -> VideoSupport {
    let Some(version) = run_ok(&["-version"]) else {
        return VideoSupport {
            available: false,
            reason: Some(
                "Saving a song as a video needs ffmpeg, which isn't installed. \
                 Everything else in Aria works without it."
                    .into(),
            ),
        };
    };
    let _ = version;

    // The encoder matters as much as the binary: minimal builds ship without it.
    let encoders = run_ok(&["-hide_banner", "-encoders"]).unwrap_or_default();
    if !encoders.contains("libx264") {
        return VideoSupport {
            available: false,
            reason: Some(
                "The ffmpeg on this computer was built without H.264 video \
                 support, so Aria can't make a video with it."
                    .into(),
            ),
        };
    }
    VideoSupport { available: true, reason: None }
}

fn run_ok(args: &[&str]) -> Option<String> {
    let out = Command::new("ffmpeg").args(args).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// How long the audio actually is, straight from the file.
///
/// Asked of ffprobe rather than taken from the library, because this value
/// decides where the video ends: the library's figure is what the engine
/// reported, and being a fraction of a second out would either clip the last
/// note or leave silence hanging on the end.
fn probe_seconds(audio: &Path) -> Option<f64> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(audio)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().ok().filter(|d| *d > 0.0)
}

/// Write an MP4 of `audio` over `cover_png`.
///
/// `cover_png` is passed as bytes rather than a path so the caller keeps
/// ownership of where — if anywhere — the artwork gets stored.
pub fn write_mp4(audio: &Path, cover_png: &[u8], dest: &Path) -> Result<()> {
    if !audio.exists() {
        bail!("That song's file isn't where Aria left it.");
    }

    // A named temporary beside the destination, cleaned up either way.
    let still = std::env::temp_dir().join(format!("aria-cover-{}.png", uuid::Uuid::new_v4()));
    std::fs::write(&still, cover_png).with_context(|| format!("writing {}", still.display()))?;

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-hide_banner", "-loglevel", "error"])
        // A single still, looped for as long as the audio lasts. Decoded at 2
        // fps and duplicated up to the output rate, which is ~15x less PNG
        // decoding than looping at 30.
        .args(["-loop", "1", "-framerate", "2"])
        .arg("-i").arg(&still)
        .arg("-i").arg(audio);

    // `-shortest` alone does not cut a looped image reliably: measured against
    // a 30.000s song it produced a 30.000s audio stream and a *32.300s* video
    // one, leaving 2.3 seconds of silent freeze-frame on the end. An explicit
    // duration is what actually makes the two agree.
    if let Some(secs) = probe_seconds(audio) {
        cmd.args(["-t", &format!("{secs:.3}")]);
    }

    let result = cmd
        .args(["-c:v", "libx264", "-tune", "stillimage", "-preset", "medium", "-crf", "20"])
        .args(["-r", &FPS.to_string()])
        // yuv420p is the pixel format every player and platform accepts; x264's
        // default 4:4:4 silently fails to play in QuickTime and most browsers.
        .args(["-pix_fmt", "yuv420p"])
        // Even dimensions are required by yuv420p; SIZE is even, but scaling
        // defensively costs nothing and survives a future size change.
        .args(["-vf", &format!("scale={SIZE}:{SIZE}:flags=lanczos")])
        // Re-encoded rather than copied: MP3-in-MP4 is legal but plays badly in
        // the places people actually post videos. The original audio file is
        // untouched and remains the master.
        .args(["-c:a", "aac", "-b:a", "320k"])
        // Stop when the audio does, not when the looped image does.
        .args(["-shortest"])
        // Puts the index at the front so the file streams before it's fully
        // downloaded — what every upload target wants.
        .args(["-movflags", "+faststart"])
        .arg(dest)
        .output();

    let _ = std::fs::remove_file(&still);

    let out = result.context("running ffmpeg")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err.lines().rev().take(4).collect::<Vec<_>>().join(" ");
        bail!("ffmpeg couldn't make the video: {}", tail.trim());
    }
    if !dest.exists() {
        bail!("ffmpeg reported success but wrote no file.");
    }
    Ok(())
}

/// The video's cover, at the size the encoder wants.
pub fn cover_png(track_id: &str) -> Vec<u8> {
    crate::art::png_for(track_id, SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_song_is_refused_before_ffmpeg_is_even_started() {
        let dir = std::env::temp_dir().join(format!("aria-vid-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = write_mp4(&dir.join("nope.mp3"), &[], &dir.join("out.mp4"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("isn't where Aria left it"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn support_reports_a_reason_whenever_it_says_no() {
        // Whatever this machine has, the contract holds: unavailable always
        // carries something to show the user, available never does.
        let s = support();
        assert_eq!(s.available, s.reason.is_none());
    }

    #[test]
    fn the_cover_is_a_png_at_the_encoder_s_size() {
        let png = cover_png("track-abc");
        assert_eq!(&png[1..4], b"PNG");
        // IHDR carries the dimensions, big-endian, right after the signature.
        assert_eq!(&png[16..20], &SIZE.to_be_bytes());
        assert_eq!(&png[20..24], &SIZE.to_be_bytes());
    }
}
