//! Getting your music out.
//!
//! Aria's promise is that the songs are yours as ordinary files, and the
//! library folder already honours that — but everything in it is named by uuid,
//! because that is what makes the index reliable. A uuid is a fine primary key
//! and a terrible thing to hand to someone who wants to put a playlist on a
//! phone.
//!
//! So export copies, in order, under names a person can read, with the covers
//! beside them and an `.m3u` any music player already understands. Nothing here
//! is Aria-specific: the point is that the result needs Aria never again.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::art;
use crate::library::Track;

/// Longest a single title may contribute to a filename. Well under the usual
/// 255-byte limit, leaving room for the number, the extension, and a deep
/// destination path.
const MAX_TITLE: usize = 80;

#[derive(Debug, Serialize)]
pub struct ExportReport {
    pub folder: String,
    pub written: usize,
    /// Titles whose audio was not where the library expected it. Reported
    /// rather than failed on: one moved file should not cancel the other
    /// nineteen songs.
    pub skipped: Vec<String>,
    pub playlist_file: Option<String>,
}

/// Copy `tracks` into `dest`, in the order given.
///
/// `m3u_name`, when present, also writes a playlist file listing them.
pub fn export(tracks: &[Track], dest: &Path, m3u_name: Option<&str>) -> Result<ExportReport> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating {}", dest.display()))?;

    // Wide enough that the files sort correctly in any file manager, which is
    // the whole reason for numbering them.
    let width = tracks.len().to_string().len().max(2);

    let mut written = 0usize;
    let mut skipped = Vec::new();
    let mut entries: Vec<(String, f64, String)> = Vec::new();

    for (i, t) in tracks.iter().enumerate() {
        let source = Path::new(&t.audio_path);
        if !source.exists() {
            skipped.push(t.title.clone());
            continue;
        }
        let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("mp3");
        let stem = format!("{:0width$} - {}", i + 1, safe_filename(&t.title), width = width);

        let audio_name = format!("{stem}.{ext}");
        std::fs::copy(source, dest.join(&audio_name))
            .with_context(|| format!("copying {} to {}", source.display(), dest.display()))?;

        // The cover travels with the song. It is regenerated rather than copied
        // so it is present even for tracks whose art was never written to disk.
        let _ = std::fs::write(dest.join(format!("{stem}.svg")), art::svg_for(&t.id));

        entries.push((audio_name, t.duration, t.title.clone()));
        written += 1;
    }

    let playlist_file = match m3u_name.filter(|_| !entries.is_empty()) {
        Some(name) => {
            let file = format!("{}.m3u", safe_filename(name));
            std::fs::write(dest.join(&file), m3u(&entries))
                .with_context(|| format!("writing {file}"))?;
            Some(file)
        }
        None => None,
    };

    Ok(ExportReport {
        folder: dest.display().to_string(),
        written,
        skipped,
        playlist_file,
    })
}

/// Extended M3U, with relative paths so the folder can be moved or copied
/// anywhere and still play.
fn m3u(entries: &[(String, f64, String)]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for (file, duration, title) in entries {
        let secs = if duration.is_finite() && *duration > 0.0 {
            duration.round() as i64
        } else {
            -1
        };
        out.push_str(&format!("#EXTINF:{secs},{title}\n{file}\n"));
    }
    out
}

/// Turn a song title into something every filesystem will accept.
///
/// Deliberately conservative: the result has to survive being copied from Linux
/// to a FAT-formatted phone to Windows, and a file that won't copy is worse
/// than one whose name lost a colon.
pub fn safe_filename(title: &str) -> String {
    let mut cleaned = String::with_capacity(title.len());
    for c in title.chars() {
        match c {
            // Reserved on Windows, and `/` on everything.
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => cleaned.push(' '),
            // Control characters, including the newline a pasted title can carry.
            c if c.is_control() => cleaned.push(' '),
            c => cleaned.push(c),
        }
    }

    // Collapse the runs those replacements just created.
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");

    // Windows silently strips trailing dots and spaces, which turns two
    // distinct titles into one filename.
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c.is_whitespace());

    let capped: String = trimmed.chars().take(MAX_TITLE).collect();
    let capped = capped.trim_end().to_string();

    if capped.is_empty() {
        "Untitled".to_string()
    } else {
        capped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, title: &str, dir: &Path) -> Track {
        let audio_path = dir.join(format!("{id}.mp3"));
        std::fs::write(&audio_path, b"audio").unwrap();
        Track {
            id: id.into(),
            title: title.into(),
            prompt: String::new(),
            caption: String::new(),
            lyrics: String::new(),
            bpm: None,
            keyscale: None,
            timesignature: None,
            vocal_language: None,
            duration: 61.4,
            seed: None,
            model: String::new(),
            audio_path: audio_path.display().to_string(),
            latent_path: None,
            created_at: 0,
            parent_id: None,
            operation: None,
            favorite: false,
            lyricist: None,
            missing: false,
        }
    }

    #[test]
    fn names_survive_every_filesystem_they_might_land_on() {
        assert_eq!(safe_filename("AC/DC: Live?"), "AC DC Live");
        assert_eq!(safe_filename("  spaced  out  "), "spaced out");
        assert_eq!(safe_filename("trailing dots..."), "trailing dots");
        assert_eq!(safe_filename("line\nbreak"), "line break");
        assert_eq!(safe_filename(""), "Untitled");
        assert_eq!(safe_filename("///"), "Untitled");
        // Non-Latin titles are not mangled; they are legal everywhere modern.
        assert_eq!(safe_filename("Waiata māori"), "Waiata māori");
        assert!(safe_filename(&"x".repeat(200)).chars().count() <= MAX_TITLE);
    }

    #[test]
    fn exports_in_order_with_covers_and_a_playlist() {
        let dir = std::env::temp_dir().join(format!("aria-export-{}", uuid::Uuid::new_v4()));
        let src = dir.join("src");
        let dest = dir.join("out");
        std::fs::create_dir_all(&src).unwrap();

        let tracks = vec![
            track("a", "First: song", &src),
            track("b", "Second song", &src),
        ];
        let report = export(&tracks, &dest, Some("Late night")).unwrap();

        assert_eq!(report.written, 2);
        assert!(report.skipped.is_empty());
        assert!(dest.join("01 - First song.mp3").exists());
        assert!(dest.join("02 - Second song.mp3").exists());
        // The cover travels with the song.
        assert!(dest.join("01 - First song.svg").exists());

        let list = std::fs::read_to_string(dest.join("Late night.m3u")).unwrap();
        assert!(list.starts_with("#EXTM3U\n"));
        // Relative paths, so the folder still plays after it is moved.
        assert!(list.contains("\n01 - First song.mp3\n"));
        assert!(list.contains("#EXTINF:61,First: song"));
        // Order is the order it was given, not alphabetical.
        assert!(list.find("01 - First").unwrap() < list.find("02 - Second").unwrap());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_moved_file_is_reported_not_fatal() {
        let dir = std::env::temp_dir().join(format!("aria-export-{}", uuid::Uuid::new_v4()));
        let src = dir.join("src");
        let dest = dir.join("out");
        std::fs::create_dir_all(&src).unwrap();

        let mut tracks = vec![track("a", "Here", &src), track("b", "Gone", &src)];
        std::fs::remove_file(&tracks[1].audio_path).unwrap();

        let report = export(&tracks, &dest, Some("Mixed")).unwrap();
        assert_eq!(report.written, 1);
        assert_eq!(report.skipped, vec!["Gone".to_string()]);

        // Numbering follows position in the request, so a skipped track leaves
        // a gap rather than renumbering everything after it.
        assert!(dest.join("01 - Here.mp3").exists());
        assert!(!dest.join("02 - Gone.mp3").exists());

        // With nothing exportable at all there is no playlist file to write.
        tracks.remove(0);
        let empty = export(&tracks, &dest.join("empty"), Some("Mixed")).unwrap();
        assert_eq!(empty.written, 0);
        assert!(empty.playlist_file.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }
}
