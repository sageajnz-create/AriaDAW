//! HTTP client for the acestep.cpp server.
//!
//! Generation is two stages. `/lm` turns a caption into lyrics, musical metadata
//! and audio codes; `/synth` renders those codes to audio. Both are async jobs:
//! POST returns an id, then you poll `/job?id=...` until it reports `done`.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// Fixed boundary used by the engine for multipart responses.
const BOUNDARY: &[u8] = b"--ace-batch-boundary";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobStatus {
    #[allow(dead_code)]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobStatus::Done | JobStatus::Failed | JobStatus::Cancelled
        )
    }
}

/// A rendered track: the audio plus the latent the engine cached for it.
/// Keeping the latent lets later repaint/cover work skip a VAE encode.
pub struct SynthOutput {
    pub audio: Vec<u8>,
    pub latent: Option<Vec<u8>>,
}

/// Audio handed to a synth job: the source being transformed, and/or a
/// reference whose voice and timbre should be borrowed.
#[derive(Default)]
pub struct SynthSources {
    pub audio: Option<Vec<u8>>,
    pub latent: Option<Vec<u8>>,
    /// Voice reference — the engine takes timbre from this without using its
    /// notes or words.
    pub ref_audio: Option<Vec<u8>>,
    pub ref_latent: Option<Vec<u8>>,
}

pub struct AceClient {
    base: String,
    http: reqwest::blocking::Client,
}

impl AceClient {
    pub fn new(base: impl Into<String>) -> Result<Self> {
        Ok(Self {
            base: base.into(),
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(600))
                .build()?,
        })
    }

    #[allow(dead_code)]
    pub fn health(&self) -> bool {
        self.http
            .get(format!("{}/health", self.base))
            .timeout(Duration::from_secs(3))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Models the engine can see, plus its default parameters.
    #[allow(dead_code)]
    pub fn props(&self) -> Result<Value> {
        Ok(self
            .http
            .get(format!("{}/props", self.base))
            .send()?
            .json()?)
    }

    fn submit(&self, endpoint: &str, req: &Value) -> Result<String> {
        let resp = self
            .http
            .post(format!("{}/{endpoint}", self.base))
            .json(req)
            .send()
            .with_context(|| format!("submitting {endpoint} job"))?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "{endpoint} rejected the request: {}",
                resp.status()
            ));
        }
        let v: Value = resp.json()?;
        v.get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("{endpoint} did not return a job id"))
    }

    pub fn submit_lm(&self, req: &Value) -> Result<String> {
        self.submit("lm", req)
    }

    pub fn submit_synth(&self, req: &Value) -> Result<String> {
        self.submit("synth", req)
    }

    /// Submit a synth job with source and/or reference audio — cover, repaint,
    /// extend, stem extraction, or generation that borrows a voice.
    ///
    /// Prefers cached latents over audio files: the engine hands back the latent
    /// it produced for every track, and replaying it skips a VAE encode.
    pub fn submit_synth_with_source(&self, req: &Value, sources: SynthSources) -> Result<String> {
        use reqwest::blocking::multipart::{Form, Part};

        let mut form = Form::new().part(
            "request",
            Part::bytes(serde_json::to_vec(req)?)
                .file_name("request.json")
                .mime_str("application/json")?,
        );

        let SynthSources {
            audio,
            latent,
            ref_audio,
            ref_latent,
        } = sources;

        // A source is only required when the task transforms existing audio; a
        // plain generation with a voice reference has none.
        match (latent, audio) {
            (Some(l), _) => {
                form = form.part(
                    "src_latents",
                    Part::bytes(l)
                        .file_name("source.latent")
                        .mime_str("application/octet-stream")?,
                )
            }
            (None, Some(a)) => {
                form = form.part(
                    "audio",
                    Part::bytes(a)
                        .file_name("source.mp3")
                        .mime_str("audio/mpeg")?,
                )
            }
            (None, None) => {}
        }

        match (ref_latent, ref_audio) {
            (Some(l), _) => {
                form = form.part(
                    "ref_latents",
                    Part::bytes(l)
                        .file_name("voice.latent")
                        .mime_str("application/octet-stream")?,
                )
            }
            (None, Some(a)) => {
                form = form.part(
                    "ref_audio",
                    Part::bytes(a)
                        .file_name("voice.mp3")
                        .mime_str("audio/mpeg")?,
                )
            }
            (None, None) => {}
        }

        let resp = self
            .http
            .post(format!("{}/synth", self.base))
            .multipart(form)
            .send()
            .context("submitting synth job with source audio")?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "the engine rejected the request: {}",
                resp.status()
            ));
        }
        let v: Value = resp.json()?;
        v.get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("synth did not return a job id"))
    }

    /// Poll a job. An empty or unreachable response means the engine died —
    /// on AMD that is usually a lost GPU device, so we surface it distinctly
    /// rather than as a generic failure.
    pub fn poll(&self, id: &str) -> Result<JobStatus> {
        let resp = self
            .http
            .get(format!("{}/job?id={id}", self.base))
            .timeout(Duration::from_secs(10))
            .send()
            .map_err(|e| anyhow!("engine unreachable while polling: {e}"))?;
        let text = resp.text()?;
        if text.trim().is_empty() {
            return Err(anyhow!("engine stopped responding"));
        }
        let v: Value = serde_json::from_str(&text)
            .map_err(|_| anyhow!("engine returned a malformed status"))?;
        match v.get("status").and_then(Value::as_str) {
            Some("done") => Ok(JobStatus::Done),
            Some("running") => Ok(JobStatus::Running),
            Some("queued") => Ok(JobStatus::Queued),
            Some("failed") => Ok(JobStatus::Failed),
            Some("cancelled") => Ok(JobStatus::Cancelled),
            other => Err(anyhow!("unknown job status: {other:?}")),
        }
    }

    /// The enriched requests produced by `/lm`: lyrics, bpm, key, audio codes.
    ///
    /// One entry per variation. With `lm_batch_size > 1` the engine returns
    /// several takes of the same prompt, each with its own codes — different
    /// performances rather than different songs.
    pub fn lm_results(&self, id: &str) -> Result<Vec<Value>> {
        let v: Value = self
            .http
            .get(format!("{}/job?id={id}&result=1", self.base))
            .send()?
            .json()?;
        Ok(match v {
            Value::Array(items) if !items.is_empty() => items,
            Value::Array(_) => return Err(anyhow!("the engine returned no results")),
            other => vec![other],
        })
    }

    pub fn synth_result(&self, id: &str) -> Result<SynthOutput> {
        let bytes = self
            .http
            .get(format!("{}/job?id={id}&result=1", self.base))
            .send()?
            .bytes()?
            .to_vec();
        parse_multipart(&bytes)
    }

    /// Analyse existing audio: tempo, key, a caption describing how it sounds,
    /// the lyrics it can hear, and the latent for reuse.
    pub fn submit_understand(&self, audio: Vec<u8>, filename: &str) -> Result<String> {
        use reqwest::blocking::multipart::{Form, Part};
        let form = Form::new().part(
            "audio",
            Part::bytes(audio)
                .file_name(filename.to_string())
                .mime_str("application/octet-stream")?,
        );
        let resp = self
            .http
            .post(format!("{}/understand", self.base))
            .multipart(form)
            .send()
            .context("submitting understand job")?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "the engine could not read that file: {}",
                resp.status()
            ));
        }
        let v: Value = resp.json()?;
        v.get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("understand did not return a job id"))
    }

    /// Result of `/understand`: one JSON part plus the source latent.
    pub fn understand_result(&self, id: &str) -> Result<(Value, Option<Vec<u8>>)> {
        let bytes = self
            .http
            .get(format!("{}/job?id={id}&result=1", self.base))
            .send()?
            .bytes()?
            .to_vec();

        let mut json_part: Option<Value> = None;
        let mut latent: Option<Vec<u8>> = None;
        for part in split_on(&bytes, BOUNDARY) {
            let Some(hdr_end) = find(part, b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&part[..hdr_end]).to_ascii_lowercase();
            let mut body = &part[hdr_end + 4..];
            if body.ends_with(b"\r\n") {
                body = &body[..body.len() - 2];
            }
            if body.is_empty() {
                continue;
            }
            if headers.contains("application/json") {
                let v: Value = serde_json::from_slice(body)?;
                json_part = Some(match v {
                    Value::Array(mut a) if !a.is_empty() => a.remove(0),
                    other => other,
                });
            } else {
                latent = Some(body.to_vec());
            }
        }
        Ok((
            json_part.ok_or_else(|| anyhow!("the engine returned no analysis"))?,
            latent,
        ))
    }

    #[allow(dead_code)]
    pub fn cancel(&self, id: &str) -> Result<()> {
        let _ = self
            .http
            .post(format!("{}/cancel?id={id}", self.base))
            .send();
        Ok(())
    }
}

/// Split the engine's `multipart/mixed` reply into its audio and latent parts.
fn parse_multipart(data: &[u8]) -> Result<SynthOutput> {
    let mut audio = None;
    let mut latent = None;

    for part in split_on(data, BOUNDARY) {
        let Some(hdr_end) = find(part, b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&part[..hdr_end]).to_ascii_lowercase();
        let mut body = &part[hdr_end + 4..];
        // Trim the CRLF the sender puts before the next boundary.
        if body.ends_with(b"\r\n") {
            body = &body[..body.len() - 2];
        }
        if body.len() < 64 {
            continue;
        }
        if headers.contains("audio/") {
            audio = Some(body.to_vec());
        } else if headers.contains("latent") || headers.contains("octet-stream") {
            latent = Some(body.to_vec());
        }
    }

    Ok(SynthOutput {
        audio: audio.ok_or_else(|| anyhow!("engine reply contained no audio"))?,
        latent,
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn split_on<'a>(data: &'a [u8], sep: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    while let Some(pos) = find(&data[start..], sep) {
        let abs = start + pos;
        if abs > start {
            out.push(&data[start..abs]);
        }
        start = abs + sep.len();
    }
    if start < data.len() {
        out.push(&data[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_audio_and_latent_parts() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"--ace-batch-boundary\r\nContent-Type: audio/mpeg\r\n\r\n");
        buf.extend_from_slice(&[0xAAu8; 128]);
        buf.extend_from_slice(b"\r\n--ace-batch-boundary\r\nContent-Type: application/octet-stream\r\nContent-Disposition: form-data; name=\"latent\"\r\n\r\n");
        buf.extend_from_slice(&[0xBBu8; 256]);
        buf.extend_from_slice(b"\r\n--ace-batch-boundary--");

        let out = parse_multipart(&buf).expect("should parse");
        assert_eq!(out.audio.len(), 128);
        assert_eq!(out.latent.as_ref().unwrap().len(), 256);
    }

    #[test]
    fn errors_when_no_audio_part() {
        let buf = b"--ace-batch-boundary\r\nContent-Type: text/plain\r\n\r\nnope\r\n--ace-batch-boundary--";
        assert!(parse_multipart(buf).is_err());
    }

    /// A real HTTP server on a real port, so the client's URLs, timeouts,
    /// status handling and JSON shapes are exercised end to end — not just
    /// the parsers. `handler` answers one request per call; the server serves
    /// up to `max_requests` and then shuts down, so its port is released.
    fn with_engine(
        max_requests: usize,
        handler: impl Fn(tiny_http::Request) + Send + 'static,
    ) -> (
        AceClient,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::thread::JoinHandle<()>,
    ) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = served.clone();
        let t = std::thread::spawn(move || {
            for (i, req) in server.incoming_requests().enumerate() {
                handler(req);
                counter.store(i + 1, std::sync::atomic::Ordering::SeqCst);
                if i + 1 >= max_requests {
                    break; // drop server → release the port, end the thread
                }
            }
        });
        (
            AceClient::new(format!("http://127.0.0.1:{port}")).unwrap(),
            served,
            t,
        )
    }

    /// Wait until the test's request(s) have actually been answered, so a join
    /// can never race a response that is still in flight.
    fn wait_served(served: &std::sync::Arc<std::sync::atomic::AtomicUsize>, n: usize) {
        for _ in 0..200 {
            if served.load(std::sync::atomic::Ordering::SeqCst) >= n {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("engine stub never received {n} request(s)");
    }

    fn reply(mut req: tiny_http::Request, status: u16, body: &str) {
        // Drain the request body first: an unanswered POST body can stall the
        // exchange on some platforms.
        std::io::copy(&mut *req.as_reader(), &mut std::io::sink()).ok();
        let resp = tiny_http::Response::from_string(body).with_status_code(status);
        let _ = req.respond(resp);
    }

    #[test]
    fn submit_posts_json_and_reads_the_job_id() {
        let (client, served, t) = with_engine(1, |mut req| {
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body).unwrap();
            assert!(
                body.contains("\"task\""),
                "engine should receive the request json, got {body}"
            );
            assert_eq!(req.url(), "/lm");
            reply(req, 200, r#"{"id":"job-1"}"#);
        });

        let id = client
            .submit_lm(&serde_json::json!({"task": "text2music", "prompt": "folk"}))
            .unwrap();
        assert_eq!(id, "job-1");
        wait_served(&served, 1);
        t.join().unwrap();
    }

    #[test]
    fn a_rejected_job_reports_who_said_no() {
        let (client, served, t) = with_engine(1, |req| reply(req, 400, "bad request"));
        let err = client
            .submit_synth(&serde_json::json!({}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("synth"), "{err}");
        wait_served(&served, 1);
        t.join().unwrap();
    }

    #[test]
    fn a_response_without_a_job_id_is_an_error_not_a_panic() {
        let (client, served, t) = with_engine(1, |req| reply(req, 200, "{}"));
        assert!(client.submit_lm(&serde_json::json!({})).is_err());
        wait_served(&served, 1);
        t.join().unwrap();
    }

    #[test]
    fn poll_maps_every_status_word() {
        for (word, want) in [
            ("done", JobStatus::Done),
            ("running", JobStatus::Running),
            ("queued", JobStatus::Queued),
            ("failed", JobStatus::Failed),
            ("cancelled", JobStatus::Cancelled),
        ] {
            let (client, served, t) = with_engine(1, move |req| {
                reply(req, 200, &format!(r#"{{"status":"{word}"}}"#))
            });
            assert_eq!(client.poll("j").unwrap(), want, "status word {word:?}");
            wait_served(&served, 1);
            t.join().unwrap();
        }
    }

    #[test]
    fn poll_treats_an_empty_body_as_a_dead_engine() {
        let (client, served, t) = with_engine(1, |req| reply(req, 200, ""));
        let err = client.poll("j").unwrap_err().to_string();
        assert!(err.contains("stopped responding"), "{err}");
        wait_served(&served, 1);
        t.join().unwrap();
    }

    #[test]
    fn poll_refuses_an_unknown_status_word_rather_than_guessing() {
        let (client, served, t) = with_engine(1, |req| reply(req, 200, r#"{"status":"warp"}"#));
        assert!(client.poll("j").is_err());
        wait_served(&served, 1);
        t.join().unwrap();
    }

    #[test]
    fn lm_results_normalises_single_and_batch_shapes() {
        // The batch shape is what the engine actually sends.
        let (client, _served, t) = with_engine(1, |req| {
            assert_eq!(req.url(), "/job?id=j1&result=1");
            reply(req, 200, r#"[{"codes":[1]},{"codes":[2]}]"#);
        });
        assert_eq!(client.lm_results("j1").unwrap().len(), 2);
        t.join().unwrap();

        // A single object is wrapped so callers never branch.
        let (client, _served, t) = with_engine(1, |req| reply(req, 200, r#"{"codes":[9]}"#));
        assert_eq!(client.lm_results("j2").unwrap().len(), 1);
        t.join().unwrap();

        // An empty array is "the engine made nothing", which must be an error.
        let (client, _served, t) = with_engine(1, |req| reply(req, 200, "[]"));
        assert!(client.lm_results("j3").is_err());
        t.join().unwrap();
    }

    #[test]
    fn synth_result_splits_audio_from_latent() {
        let mut body = Vec::new();
        body.extend_from_slice(b"--ace-batch-boundary\r\nContent-Type: audio/mpeg\r\n\r\n");
        let audio = vec![0xAAu8; 100];
        body.extend_from_slice(&audio);
        body.extend_from_slice(b"\r\n--ace-batch-boundary\r\nContent-Type: application/octet-stream; name=\"latent\"\r\n\r\n");
        let latent = vec![0xBBu8; 80];
        body.extend_from_slice(&latent);
        body.extend_from_slice(b"\r\n--ace-batch-boundary--");

        let (client, _served, t) = with_engine(1, move |req| {
            let mut b = body.clone();
            let mut resp = tiny_http::Response::from_data(std::mem::take(&mut b));
            resp.add_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"multipart/mixed"[..])
                    .unwrap(),
            );
            let _ = req.respond(resp);
        });
        let out = client.synth_result("j").unwrap();
        assert_eq!(out.audio, audio);
        assert_eq!(out.latent.unwrap(), latent);
        t.join().unwrap();
    }
}
