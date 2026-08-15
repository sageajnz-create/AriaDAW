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
    pub fn is_terminal(self) -> bool {
        matches!(self, JobStatus::Done | JobStatus::Failed | JobStatus::Cancelled)
    }
}

/// A rendered track: the audio plus the latent the engine cached for it.
/// Keeping the latent lets later repaint/cover work skip a VAE encode.
pub struct SynthOutput {
    pub audio: Vec<u8>,
    pub latent: Option<Vec<u8>>,
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

    pub fn health(&self) -> bool {
        self.http
            .get(format!("{}/health", self.base))
            .timeout(Duration::from_secs(3))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Models the engine can see, plus its default parameters.
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
            return Err(anyhow!("{endpoint} rejected the request: {}", resp.status()));
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

    /// Submit a synth job that works from existing audio — cover, repaint,
    /// extend, stem extraction.
    ///
    /// Prefers the cached latent over the audio file when we have one: the
    /// engine hands back the latent it produced for every track, and replaying
    /// it skips a VAE encode entirely.
    pub fn submit_synth_with_source(
        &self,
        req: &Value,
        audio: Option<Vec<u8>>,
        latent: Option<Vec<u8>>,
    ) -> Result<String> {
        use reqwest::blocking::multipart::{Form, Part};

        let mut form = Form::new().part(
            "request",
            Part::bytes(serde_json::to_vec(req)?)
                .file_name("request.json")
                .mime_str("application/json")?,
        );

        form = match (latent, audio) {
            (Some(l), _) => form.part(
                "src_latents",
                Part::bytes(l)
                    .file_name("source.latent")
                    .mime_str("application/octet-stream")?,
            ),
            (None, Some(a)) => form.part(
                "audio",
                Part::bytes(a).file_name("source.mp3").mime_str("audio/mpeg")?,
            ),
            (None, None) => return Err(anyhow!("this needs a source track")),
        };

        let resp = self
            .http
            .post(format!("{}/synth", self.base))
            .multipart(form)
            .send()
            .context("submitting synth job with source audio")?;
        if !resp.status().is_success() {
            return Err(anyhow!("the engine rejected the request: {}", resp.status()));
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

    /// The enriched request produced by `/lm`: lyrics, bpm, key, audio codes.
    pub fn lm_result(&self, id: &str) -> Result<Value> {
        let v: Value = self
            .http
            .get(format!("{}/job?id={id}&result=1", self.base))
            .send()?
            .json()?;
        // Batch jobs return an array; we submit one at a time.
        Ok(match v {
            Value::Array(mut items) if !items.is_empty() => items.remove(0),
            other => other,
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
            return Err(anyhow!("the engine could not read that file: {}", resp.status()));
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
            let Some(hdr_end) = find(part, b"\r\n\r\n") else { continue };
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
        let Some(hdr_end) = find(part, b"\r\n\r\n") else { continue };
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
}
