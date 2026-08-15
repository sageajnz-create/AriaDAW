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
    Full { path: PathBuf, len: u64 },
    Partial { path: PathBuf, start: u64, end: u64, total: u64 },
    Denied(u16, String),
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
    let id = match rest.strip_prefix("track/") {
        Some(i) if !i.is_empty() => i,
        _ => return Reply::Denied(404, "not a track url".into()),
    };

    let track = match library.lock().unwrap().get(id) {
        Ok(Some(t)) => t,
        Ok(None) => return Reply::Denied(404, "no such track".into()),
        Err(e) => return Reply::Denied(500, format!("library error: {e}")),
    };

    let path = PathBuf::from(&track.audio_path);
    let total = match std::fs::metadata(&path) {
        Ok(m) => m.len(),
        Err(e) => return Reply::Denied(404, format!("cannot read file: {e}")),
    };

    match range.and_then(|r| parse_range(r, total)) {
        Some((start, end)) => Reply::Partial { path, start, end, total },
        None => Reply::Full { path, len: total },
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
    let audio = Header::from_bytes(&b"Content-Type"[..], &b"audio/mpeg"[..]).unwrap();
    let ranges = Header::from_bytes(&b"Accept-Ranges"[..], &b"bytes"[..]).unwrap();

    match reply {
        Reply::Full { path, len } => {
            let file = std::fs::File::open(&path)?;
            let resp = Response::new(StatusCode(200), vec![audio, ranges], file, Some(len as usize), None);
            request.respond(resp)
        }
        Reply::Partial { path, start, end, total } => {
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
