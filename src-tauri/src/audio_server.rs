//! A tiny localhost HTTP server for playing tracks.
//!
//! Why this exists, after three failed approaches:
//!
//! WebKitGTK does not decode media itself — it hands the URL to GStreamer, and
//! GStreamer resolves it with a URI handler. There is a handler for `http`,
//! `file` and `blob`, but none for a custom scheme, so `aria://` never reaches
//! a decoder no matter how correct the response is. The engine log confirmed
//! Rust was serving whole 8 MB files successfully while the player still
//! reported failure.
//!
//! Tauri's asset protocol and blob URLs fail for related reasons. Plain HTTP on
//! loopback is the one transport GStreamer opens without argument, and it
//! brings working range requests, so seeking behaves.
//!
//! Scope: binds to 127.0.0.1 only, and every URL carries a token generated
//! fresh each run, so another local process can't enumerate the library.

use std::io::{Read, Seek, SeekFrom};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tiny_http::{Header, Response, Server, StatusCode};

use crate::library::Library;

pub struct AudioServer {
    pub port: u16,
    pub token: String,
}

impl AudioServer {
    /// Bind to a free loopback port and serve in the background.
    pub fn start(library: Arc<Mutex<Library>>) -> Result<Self> {
        // Port 0 lets the OS pick a free one, avoiding clashes.
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        let token = uuid::Uuid::new_v4().to_string();

        let server = Server::from_listener(listener, None)
            .map_err(|e| anyhow::anyhow!("could not start the audio server: {e}"))?;

        let expected = token.clone();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let url = request.url().to_string();
                let range = header_value(&request, "range");
                let resp = handle(&url, range.as_deref(), &expected, &library);
                if let Err(e) = respond(request, resp) {
                    eprintln!("[audio] failed to respond: {e}");
                }
            }
        });

        Ok(Self { port, token })
    }

    /// Base URL the webview should use.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/{}", self.port, self.token)
    }
}

fn header_value(request: &tiny_http::Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

enum Reply {
    Full { path: PathBuf, len: u64, mime: &'static str },
    /// Generated on the spot rather than read from disk — cover art.
    Inline { body: String, mime: &'static str },
    Partial { path: PathBuf, start: u64, end: u64, total: u64, mime: &'static str },
    Denied(u16, String),
}

/// Tracks can be saved lossless, so the type has to follow the file rather than
/// always claiming mp3 — a wav served as audio/mpeg won't play.
fn mime_for(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" | "opus" => "audio/ogg",
        "m4a" | "aac" => "audio/mp4",
        _ => "audio/mpeg",
    }
}

fn handle(
    url: &str,
    range: Option<&str>,
    expected_token: &str,
    library: &Arc<Mutex<Library>>,
) -> Reply {
    // /<token>/track/<id>
    let rest = match url.strip_prefix('/').and_then(|u| u.split_once('/')) {
        Some((token, rest)) if token == expected_token => rest,
        Some(_) => return Reply::Denied(403, "bad token".into()),
        None => return Reply::Denied(404, "not found".into()),
    };
    let (kind, id) = match rest.split_once('/') {
        Some((k, i)) if !i.is_empty() => (k, i),
        _ => return Reply::Denied(404, "not a track url".into()),
    };

    let track = match library.lock().unwrap().get(id) {
        Ok(Some(t)) => t,
        Ok(None) => return Reply::Denied(404, "no such track".into()),
        Err(e) => return Reply::Denied(500, format!("library error: {e}")),
    };

    if kind == "art" {
        // Drawn from the id, so it costs nothing to reproduce and never
        // depends on the audio file still being where we left it. Saving a
        // copy beside the audio is what makes it the user's, not ours.
        let svg = crate::art::svg_for(&track.id);
        crate::art::save_beside(&track.audio_path, &svg);
        return Reply::Inline { body: svg, mime: "image/svg+xml" };
    }
    if kind != "track" {
        return Reply::Denied(404, "not a track url".into());
    }

    let path = PathBuf::from(&track.audio_path);
    let total = match std::fs::metadata(&path) {
        Ok(m) => m.len(),
        Err(e) => return Reply::Denied(404, format!("cannot read file: {e}")),
    };

    let mime = mime_for(&path);
    match range.and_then(|r| parse_range(r, total)) {
        Some((start, end)) => Reply::Partial { path, start, end, total, mime },
        None => Reply::Full { path, len: total, mime },
    }
}

/// "bytes=0-" or "bytes=500-999". Only a single range, which is all a media
/// element ever asks for.
fn parse_range(raw: &str, total: u64) -> Option<(u64, u64)> {
    let spec = raw.trim().strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: u64 = a.trim().parse().ok()?;
    let end: u64 = if b.trim().is_empty() {
        total.checked_sub(1)?
    } else {
        b.trim().parse().ok()?
    };
    if start > end || start >= total {
        return None;
    }
    Some((start, end.min(total.saturating_sub(1))))
}

fn respond(request: tiny_http::Request, reply: Reply) -> std::io::Result<()> {
    let ranges = Header::from_bytes(&b"Accept-Ranges"[..], &b"bytes"[..]).unwrap();
    let ctype = |m: &str| Header::from_bytes(&b"Content-Type"[..], m.as_bytes()).unwrap();

    match reply {
        Reply::Full { path, len, mime } => {
            let audio = ctype(mime);
            let file = std::fs::File::open(&path)?;
            let resp = Response::new(StatusCode(200), vec![audio, ranges], file, Some(len as usize), None);
            request.respond(resp)
        }
        Reply::Inline { body, mime } => {
            // Deterministic for the life of the track, so the webview may keep it.
            let cache = Header::from_bytes(&b"Cache-Control"[..], &b"max-age=86400"[..]).unwrap();
            let resp = Response::from_string(body).with_header(ctype(mime)).with_header(cache);
            request.respond(resp)
        }
        Reply::Partial { path, start, end, total, mime } => {
            let audio = ctype(mime);
            let mut file = std::fs::File::open(&path)?;
            file.seek(SeekFrom::Start(start))?;
            let len = end - start + 1;
            let mut buf = vec![0u8; len as usize];
            file.read_exact(&mut buf)?;
            let content_range = Header::from_bytes(
                &b"Content-Range"[..],
                format!("bytes {start}-{end}/{total}").as_bytes(),
            )
            .unwrap();
            let resp = Response::from_data(buf)
                .with_status_code(StatusCode(206))
                .with_header(audio)
                .with_header(ranges)
                .with_header(content_range);
            request.respond(resp)
        }
        Reply::Denied(code, why) => {
            eprintln!("[audio] {code}: {why}");
            request.respond(Response::from_string(why).with_status_code(StatusCode(code)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn parses_the_ranges_media_elements_send() {
        assert_eq!(parse_range("bytes=0-", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=500-999", 1000), Some((500, 999)));
        // An end past the file is clamped rather than rejected.
        assert_eq!(parse_range("bytes=500-5000", 1000), Some((500, 999)));
    }

    #[test]
    fn rejects_nonsense_ranges() {
        assert_eq!(parse_range("bytes=999-500", 1000), None);
        assert_eq!(parse_range("bytes=2000-", 1000), None);
        assert_eq!(parse_range("items=0-10", 1000), None);
        assert_eq!(parse_range("garbage", 1000), None);
    }
}
