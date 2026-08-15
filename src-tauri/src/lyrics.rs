//! Lyric writing via a local instruct model (Ollama), when one is available.
//!
//! ACE-Step ships its own language model, but it is built to emit audio codes,
//! not to write words. Measured on the same prompt, its lyrics were 13% distinct
//! lines — "I talk talk talk talk talk" — drifted into languages nobody asked
//! for, and frequently produced only vocalisations. Lowering its sampling
//! temperature helped (13% -> 48% distinct) but did not fix language drift.
//!
//! A general instruct model is simply the right tool: `llama3.2:3b` produced
//! 100% distinct lines, in the requested language, actually about the subject,
//! in about 13 seconds.
//!
//! This is strictly optional. Without Ollama, Aria falls back to ACE-Step's own
//! writer — worse, but it still makes songs, and nothing extra is required to
//! install. Cloud-hosted Ollama models are deliberately excluded: Aria's whole
//! promise is that your words never leave your machine.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::Duration;

const OLLAMA: &str = "http://127.0.0.1:11434";

/// Local instruct models we prefer, best-fit first. Small models do this job
/// well, and smaller means faster and less VRAM contention with the audio
/// models that run immediately afterwards.
const PREFERRED: &[&str] = &[
    "llama3.2:3b",
    "gemma3:4b",
    "llama3.2",
    "qwen3:8b",
    "gemma3",
    "mistral",
    "phi3",
];

pub struct LyricWriter {
    model: String,
    http: reqwest::blocking::Client,
}

impl LyricWriter {
    /// Find a usable local model, or None if Ollama isn't running.
    pub fn detect() -> Option<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .ok()?;

        let tags: Value = http
            .get(format!("{OLLAMA}/api/tags"))
            .timeout(Duration::from_secs(2))
            .send()
            .ok()?
            .json()
            .ok()?;

        let installed: Vec<String> = tags
            .get("models")?
            .as_array()?
            .iter()
            .filter_map(|m| m.get("name")?.as_str().map(str::to_string))
            // Never route the user's words through a hosted model.
            .filter(|n| !n.contains(":cloud"))
            .collect();

        let model = PREFERRED
            .iter()
            .find_map(|p| installed.iter().find(|n| n.starts_with(p)).cloned())
            .or_else(|| installed.first().cloned())?;

        Some(Self { model, http })
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// Write lyrics for a song. `language` is a human-readable name ("English").
    pub fn write(&self, subject: &str, language: &str, seconds: f64) -> Result<String> {
        // Roughly four sung lines per ten seconds, kept within sane bounds.
        let lines = ((seconds / 10.0 * 4.0).round() as i64).clamp(8, 48);

        let system = "You write song lyrics. Output ONLY the lyrics and nothing else \
             — no title, no explanation, no notes.\n\
             Put section markers on their own lines: [Intro], [Verse 1], [Chorus], \
             [Verse 2], [Bridge], [Outro].\n\
             Rules:\n\
             - Every sung line must be real words in the requested language.\n\
             - Never write filler syllables like 'oh oh oh', 'la la la' or 'yeah yeah'.\n\
             - Do not repeat any line more than twice in the whole song.\n\
             - Keep lines short enough to sing, about 6 to 10 words.\n\
             - The lyrics must genuinely be about the subject given.";

        let user = format!(
            "Write lyrics in {language} for this song: {subject}\n\
             Around {lines} sung lines. Write them in {language}."
        );

        let body = json!({
            "model": self.model,
            "stream": false,
            // Release the VRAM straight away — the audio models need it next.
            "keep_alive": 0,
            "options": { "temperature": 0.8 },
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        });

        let resp: Value = self
            .http
            .post(format!("{OLLAMA}/api/chat"))
            .json(&body)
            .send()?
            .json()?;

        let text = resp
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("lyric model returned no content"))?;

        let cleaned = clean(text);
        if cleaned.trim().is_empty() {
            return Err(anyhow!("lyric model returned nothing usable"));
        }
        Ok(cleaned)
    }
}

/// Strip the things instruct models add despite being told not to: code fences,
/// a leading title, and any trailing commentary after the last lyric line.
fn clean(raw: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            continue;
        }
        // Commentary the model tacks on, e.g. "Here are the lyrics:".
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("here are")
            || lower.starts_with("here's")
            || lower.starts_with("note:")
            || lower.starts_with("i hope")
        {
            continue;
        }
        out.push(line);
    }
    // Trim blank lines from both ends.
    while out.first().is_some_and(|l| l.trim().is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn strips_fences_and_commentary() {
        let raw = "Here are the lyrics:\n```\n[Verse 1]\nStones by the river\n```\nI hope you like it!";
        assert_eq!(clean(raw), "[Verse 1]\nStones by the river");
    }

    #[test]
    fn keeps_plain_lyrics_untouched() {
        let raw = "[Verse 1]\nStones by the river\n\n[Chorus]\nGouda gold";
        assert_eq!(clean(raw), raw);
    }
}
