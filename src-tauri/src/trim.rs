//! Cutting a song down to a section.
//!
//! Every other derived operation asks the engine to make new audio. Trim does
//! not: it copies the bytes that were already there. That difference is worth
//! protecting, because it means a trim is instant, costs no GPU time, and — the
//! part that actually matters — cannot change how the rest of the song sounds.
//! Re-encoding a 320 kbps MP3 to cut ten seconds off the front would quietly
//! degrade the whole thing.
//!
//! So both supported formats are cut on their own natural boundaries:
//!
//! - **WAV** on a sample frame, with the original `fmt ` chunk copied verbatim
//!   so 16-, 24- and 32-bit float files all survive without being understood
//!   in detail.
//! - **MP3** on a frame header. MP3 frames are self-contained enough that
//!   dropping whole ones is the standard way to cut without decoding.
//!
//! Anything else is refused rather than mangled. Trimming a FLAC or an Opus
//! file needs a decoder for that format, and Aria does not ship one.

use std::path::Path;

use anyhow::{bail, Context, Result};

/// The shortest section worth keeping. Below this a trim is almost certainly a
/// slip of the slider.
pub const MIN_SECONDS: f64 = 1.0;

/// Cut `input` between `start` and `end` seconds, writing `output`.
///
/// Returns the duration actually produced, which lands on the nearest frame
/// boundary rather than exactly where the slider was.
pub fn trim(input: &Path, start: f64, end: f64, output: &Path) -> Result<f64> {
    if !(end - start >= MIN_SECONDS) {
        bail!("A trimmed song needs to be at least {MIN_SECONDS:.0} second long.");
    }
    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let (bytes, duration) = match ext.as_str() {
        "wav" => trim_wav(&std::fs::read(input)?, start, end)?,
        "mp3" => trim_mp3(&std::fs::read(input)?, start, end)?,
        other => bail!(
            "Aria can trim its own songs, which are MP3 or WAV. This one is {} \
             — export it and trim it in an audio editor.",
            if other.is_empty() { "in an unknown format".into() } else { format!("a {other} file") }
        ),
    };

    std::fs::write(output, &bytes)
        .with_context(|| format!("writing {}", output.display()))?;
    Ok(duration)
}

// --- WAV -----------------------------------------------------------------

/// Cut a RIFF/WAVE file on a sample-frame boundary.
///
/// The `fmt ` chunk is copied byte for byte rather than rebuilt: that way
/// 24-bit PCM, 32-bit float and WAVE_FORMAT_EXTENSIBLE files all come through
/// intact without this code having to know what any of them mean.
fn trim_wav(data: &[u8], start: f64, end: f64) -> Result<(Vec<u8>, f64)> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        bail!("That file isn't a WAV Aria can read.");
    }

    let mut fmt: Option<&[u8]> = None;
    let mut audio: Option<&[u8]> = None;
    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(size).min(data.len());
        match id {
            b"fmt " => fmt = Some(&data[body_start..body_end]),
            b"data" => audio = Some(&data[body_start..body_end]),
            _ => {}
        }
        // Chunks are padded to an even length; the pad byte is not counted in
        // the size, so walking without it desynchronises everything after.
        pos = body_start + size + (size & 1);
    }

    let fmt = fmt.context("That WAV has no format chunk.")?;
    let audio = audio.context("That WAV has no audio in it.")?;
    if fmt.len() < 16 {
        bail!("That WAV's format chunk is too short to read.");
    }

    let byte_rate = u32::from_le_bytes([fmt[8], fmt[9], fmt[10], fmt[11]]) as usize;
    let block_align = u16::from_le_bytes([fmt[12], fmt[13]]) as usize;
    if byte_rate == 0 || block_align == 0 {
        bail!("That WAV declares a rate Aria can't work with.");
    }

    // Snap to whole sample frames, or the result is offset noise.
    let snap = |secs: f64| -> usize {
        let raw = (secs.max(0.0) * byte_rate as f64) as usize;
        (raw / block_align * block_align).min(audio.len() / block_align * block_align)
    };
    let from = snap(start);
    let to = snap(end).max(from + block_align).min(audio.len());
    let cut = &audio[from..to];

    let mut out = Vec::with_capacity(cut.len() + fmt.len() + 28);
    out.extend_from_slice(b"RIFF");
    let riff_size = 4 + (8 + fmt.len()) + (8 + cut.len());
    out.extend_from_slice(&(riff_size as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    out.extend_from_slice(fmt);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(cut.len() as u32).to_le_bytes());
    out.extend_from_slice(cut);

    Ok((out, cut.len() as f64 / byte_rate as f64))
}

// --- MP3 -----------------------------------------------------------------

struct Frame {
    offset: usize,
    len: usize,
    samples: usize,
    rate: usize,
}

/// Cut an MP3 by dropping whole frames.
///
/// Any Xing/Info/VBRI header frame is dropped rather than copied: it describes
/// the length and seek table of the *original* file, so carrying it over would
/// leave every player reporting the wrong duration for the cut.
fn trim_mp3(data: &[u8], start: f64, end: f64) -> Result<(Vec<u8>, f64)> {
    let body = skip_id3v2(data);
    let frames = parse_frames(body);
    if frames.is_empty() {
        bail!("Aria couldn't find any audio in that MP3.");
    }

    // Cumulative time at the start of each frame.
    let mut times = Vec::with_capacity(frames.len());
    let mut elapsed = 0.0f64;
    for f in &frames {
        times.push(elapsed);
        elapsed += f.samples as f64 / f.rate as f64;
    }

    let skip_first = is_info_frame(body, &frames[0]) as usize;
    let first = frames
        .iter()
        .enumerate()
        .skip(skip_first)
        .find(|(i, f)| times[*i] + f.samples as f64 / f.rate as f64 > start)
        .map(|(i, _)| i)
        .unwrap_or(skip_first);
    let last = frames
        .iter()
        .enumerate()
        .skip(first)
        .take_while(|(i, _)| times[*i] < end)
        .map(|(i, _)| i)
        .last()
        .unwrap_or(first);

    let mut out = Vec::new();
    let mut duration = 0.0f64;
    for f in &frames[first..=last] {
        out.extend_from_slice(&body[f.offset..f.offset + f.len]);
        duration += f.samples as f64 / f.rate as f64;
    }
    Ok((out, duration))
}

/// ID3v2 tags sit in front of the audio and are not frames.
fn skip_id3v2(data: &[u8]) -> &[u8] {
    if data.len() < 10 || &data[0..3] != b"ID3" {
        return data;
    }
    // A syncsafe integer: 7 bits per byte, so no byte can look like a sync word.
    let size = ((data[6] as usize & 0x7f) << 21)
        | ((data[7] as usize & 0x7f) << 14)
        | ((data[8] as usize & 0x7f) << 7)
        | (data[9] as usize & 0x7f);
    let footer = if data[5] & 0x10 != 0 { 10 } else { 0 };
    let end = (10 + size + footer).min(data.len());
    &data[end..]
}

fn parse_frames(body: &[u8]) -> Vec<Frame> {
    let mut frames = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= body.len() {
        match frame_at(body, pos) {
            Some(f) => {
                pos += f.len;
                frames.push(f);
            }
            // Junk between frames (or a trailing ID3v1 tag): step forward and
            // look for the next sync rather than giving up on the file.
            None => pos += 1,
        }
    }
    frames
}

fn frame_at(body: &[u8], pos: usize) -> Option<Frame> {
    let h = body.get(pos..pos + 4)?;
    if h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (h[1] >> 3) & 0x03; // 0 = MPEG2.5, 2 = MPEG2, 3 = MPEG1
    let layer = (h[1] >> 1) & 0x03; // 1 = Layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let bitrate_index = (h[2] >> 4) as usize;
    let rate_index = ((h[2] >> 2) & 0x03) as usize;
    let padding = ((h[2] >> 1) & 0x01) as usize;
    if bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return None;
    }

    const MPEG1: [usize; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    const MPEG2: [usize; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    const RATES: [[usize; 3]; 4] = [
        [11025, 12000, 8000], // MPEG2.5
        [0, 0, 0],            // reserved
        [22050, 24000, 16000], // MPEG2
        [44100, 48000, 32000], // MPEG1
    ];

    let is_mpeg1 = version == 3;
    let bitrate = if is_mpeg1 { MPEG1[bitrate_index] } else { MPEG2[bitrate_index] } * 1000;
    let rate = RATES[version as usize][rate_index];
    if rate == 0 {
        return None;
    }

    // Layer III: MPEG1 carries 1152 samples per frame, MPEG2/2.5 half that.
    let (coefficient, samples) = if is_mpeg1 { (144, 1152) } else { (72, 576) };
    let len = coefficient * bitrate / rate + padding;
    if len < 4 || pos + len > body.len() {
        return None;
    }
    Some(Frame { offset: pos, len, samples, rate })
}

/// True when the first frame is a Xing/Info/VBRI metadata frame rather than audio.
fn is_info_frame(body: &[u8], f: &Frame) -> bool {
    let end = (f.offset + f.len).min(body.len());
    // The marker always sits in the frame's first few dozen bytes, so a bounded
    // search can't be fooled by audio data further in.
    let head = &body[f.offset..end.min(f.offset + 48)];
    head.windows(4)
        .any(|w| w == b"Xing" || w == b"Info" || w == b"VBRI")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16-bit mono at 44.1 kHz, so byte rate is 88200 and a frame is 2 bytes.
    fn wav(seconds: f64) -> Vec<u8> {
        let byte_rate = 88_200usize;
        let data_len = (byte_rate as f64 * seconds) as usize / 2 * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&44_100u32.to_le_bytes());
        out.extend_from_slice(&(byte_rate as u32).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        out.extend(std::iter::repeat(0u8).take(data_len));
        out
    }

    /// MPEG1 Layer III, 320 kbps, 44.1 kHz — exactly what the engine writes.
    /// Each frame is 1044 bytes and 1152 samples, so ~26.12 ms.
    fn mp3(frames: usize, with_xing: bool) -> Vec<u8> {
        let len = 144 * 320_000 / 44_100; // 1044
        let mut out = Vec::new();
        for i in 0..frames {
            out.extend_from_slice(&[0xFF, 0xFB, 0xE0, 0x00]);
            let mut body = vec![0u8; len - 4];
            if i == 0 && with_xing {
                body[32..36].copy_from_slice(b"Xing");
            }
            out.extend_from_slice(&body);
        }
        out
    }

    #[test]
    fn wav_is_cut_on_sample_boundaries() {
        let src = wav(4.0);
        let (out, secs) = trim_wav(&src, 1.0, 3.0).unwrap();
        assert!((secs - 2.0).abs() < 0.01, "got {secs}");
        assert_eq!(&out[0..4], b"RIFF");
        assert_eq!(&out[8..12], b"WAVE");
        // Header says what the file actually contains.
        let riff = u32::from_le_bytes([out[4], out[5], out[6], out[7]]) as usize;
        assert_eq!(riff, out.len() - 8);
        let data_len = u32::from_le_bytes([out[40], out[41], out[42], out[43]]) as usize;
        assert_eq!(data_len, out.len() - 44);
        assert_eq!(data_len % 2, 0, "must land on a whole sample frame");
    }

    #[test]
    fn a_trim_past_the_end_stops_at_the_end() {
        let src = wav(2.0);
        let (_, secs) = trim_wav(&src, 1.0, 60.0).unwrap();
        assert!((secs - 1.0).abs() < 0.01, "got {secs}");
    }

    #[test]
    fn mp3_is_cut_on_frame_boundaries() {
        let frame_secs = 1152.0 / 44_100.0;
        let src = mp3(200, false);
        let (out, secs) = trim_mp3(&src, 1.0, 3.0).unwrap();

        // Whole frames only: the output has to divide exactly.
        assert_eq!(out.len() % 1044, 0);
        assert!((secs - 2.0).abs() < frame_secs * 2.0, "got {secs}");
        // And it still starts with a valid frame header.
        assert_eq!(out[0], 0xFF);
        assert_eq!(out[1] & 0xE0, 0xE0);
    }

    #[test]
    fn the_original_files_length_header_is_not_carried_over() {
        let src = mp3(100, true);
        let (out, _) = trim_mp3(&src, 0.0, 1.0).unwrap();
        // Carrying the Xing frame across would leave players reporting the
        // length of the song we just cut down.
        assert!(!out.windows(4).any(|w| w == b"Xing"));
    }

    #[test]
    fn an_id3_tag_in_front_does_not_hide_the_audio() {
        let mut src = Vec::new();
        src.extend_from_slice(b"ID3\x03\x00\x00");
        // Syncsafe 200.
        src.extend_from_slice(&[0, 0, 1, 72]);
        src.extend(std::iter::repeat(0u8).take(200));
        src.extend_from_slice(&mp3(100, false));

        let (out, secs) = trim_mp3(&src, 0.0, 1.0).unwrap();
        assert_eq!(out[0], 0xFF);
        assert!(secs > 0.9 && secs < 1.1, "got {secs}");
    }

    #[test]
    fn formats_we_cannot_cut_are_refused_by_name() {
        let dir = std::env::temp_dir().join(format!("aria-trim-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let flac = dir.join("song.flac");
        std::fs::write(&flac, b"not really a flac").unwrap();

        let err = trim(&flac, 0.0, 5.0, &dir.join("out.flac")).unwrap_err().to_string();
        assert!(err.contains("flac"), "{err}");
        // Refused, not half-written.
        assert!(!dir.join("out.flac").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_section_shorter_than_a_second_is_refused() {
        let dir = std::env::temp_dir().join(format!("aria-trim-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.wav");
        std::fs::write(&path, wav(4.0)).unwrap();

        assert!(trim(&path, 1.0, 1.2, &dir.join("out.wav")).is_err());
        assert!(trim(&path, 1.0, 2.5, &dir.join("out.wav")).is_ok());

        std::fs::remove_dir_all(&dir).ok();
    }
}
