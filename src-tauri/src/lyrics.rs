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

    /// Expand a short style request into a detailed musical description.
    ///
    /// The engine can do this itself, but its version drifts: "a modern jazz
    /// song" came back as "a theatrical art-pop track", having taken its cue
    /// from the lyrics instead of the request. Doing it here lets us insist the
    /// stated genre survives, while still giving the audio model the detailed
    /// caption it works best from.
    pub fn expand_style(&self, style: &str, language: &str, instrumental: bool) -> Result<String> {
        let system = "You turn a short music request into one detailed paragraph \
             describing how the track sounds. Output ONLY that paragraph.\n\
             Rules:\n\
             - The genre and mood the user asked for must survive exactly. Never \
               substitute a different genre.\n\
             - Add concrete detail: instruments, rhythm, texture, production, energy.\n\
             - Describe sound only. Do not write lyrics or a title.\n\
             - Two to four sentences.";

        let voice = if instrumental {
            "It is instrumental, with no singing.".to_string()
        } else {
            format!("It has vocals sung in {language}.")
        };
        let user = format!("Music request: {style}\n{voice}");

        let body = json!({
            "model": self.model,
            "stream": false,
            "keep_alive": 0,
            "options": { "temperature": 0.7 },
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
            .ok_or_else(|| anyhow!("style model returned no content"))?;

        let cleaned = clean(text).replace('\n', " ");
        if cleaned.trim().len() < 20 {
            return Err(anyhow!("style expansion too short to be useful"));
        }
        // Keep the user's own words in front, so the request is never buried.
        Ok(format!("{}. {}", style.trim_end_matches(['.', ' ']), cleaned.trim()))
    }

    /// Write lyrics for a song. `language` is a human-readable name ("English").
    pub fn write(&self, subject: &str, language: &str, seconds: f64) -> Result<String> {
        // About eleven sung lines per minute.
        //
        // The first attempt asked for four lines per ten seconds — 19 a minute,
        // roughly double how fast people actually sing. At two and a half
        // minutes that meant 48 lines crammed into the time, leaving no room for
        // an intro, a break or a held note, and the result was reported as
        // "more of a bed than a song".
        let lines = ((seconds / 60.0 * 11.0).round() as i64).clamp(6, 40);

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

        // Naming the length and asking for instrumental space explicitly keeps
        // the words from filling every second of a longer song.
        let minutes = seconds / 60.0;
        let user = format!(
            "Write lyrics in {language} for this song: {subject}\n\
             The song is about {minutes:.1} minutes long, so write around {lines} \
             sung lines — no more. Leave room for the music: include an [Intro] \
             and at least one instrumental section such as [Instrumental Break] \
             or [Guitar Solo], and an [Outro]. Write the words in {language}."
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

/// How good a set of lyrics is, from 0 (unusable) to 1.
///
/// This exists because ACE-Step's own lyric writer is not consistently bad — it
/// is *inconsistent*. Four runs of the same prompt scored 0.05, 0.00, 0.69 and
/// 0.03: one in four was genuinely good and the rest were repetition or
/// nothing at all. A score lets us generate a few and keep the best, which
/// turns that variance into quality without needing another model.
///
/// Judged on what actually went wrong in practice: repeated lines, filler
/// syllables instead of words, and too few lines to be a song.
pub fn score_lyrics(lyrics: &str) -> f64 {
    let lines: Vec<&str> = lyrics
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('['))
        .collect();

    if lines.is_empty() {
        return 0.0;
    }

    let unique: std::collections::HashSet<&str> = lines.iter().copied().collect();
    let distinct = unique.len() as f64 / lines.len() as f64;

    let noise = lines.iter().filter(|l| is_filler(l)).count() as f64 / lines.len() as f64;

    // Below about six sung lines there isn't a song there, however varied.
    let enough = (lines.len() as f64 / 6.0).min(1.0);

    distinct * (1.0 - noise) * enough
}

/// A line that is vocalising rather than saying anything: "oh oh oh", "la la la".
fn is_filler(line: &str) -> bool {
    // Punctuation separates words rather than disappearing: dropping the
    // hyphens in "Wah-oh-oh-oh" would glue it into one unrecognised token,
    // and that exact form is what the engine actually produced.
    let cleaned: String = line
        .chars()
        .map(|c| if c.is_alphabetic() { c.to_ascii_lowercase() } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.is_empty() {
        return true;
    }
    const FILLER: &[&str] = &[
        "oh", "ooh", "ah", "aah", "yeah", "na", "la", "da", "hmm", "mmm", "woo",
        "hey", "ay", "uh", "wah", "ooo", "mm", "ha",
    ];
    words.iter().all(|w| FILLER.contains(w))
}

/// Good enough to stop looking. Set from the measurements above: the one good
/// run scored 0.69 and the failures sat below 0.06, so anything near 0.55 is
/// comfortably on the right side of that gap.
pub const GOOD_ENOUGH: f64 = 0.55;

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
    use super::{clean, is_filler, score_lyrics, GOOD_ENOUGH};

    #[test]
    fn scores_real_engine_output_the_way_a_listener_would() {
        // Taken from actual runs. The repetitive one is what a user described
        // as "just sounds and not words".
        let repetitive = "[Verse 1]\nI talk talk talk talk talk\nI talk talk talk talk talk\n\
                          I talk talk talk talk talk\nI talk talk talk talk talk";
        let vocalising = "[Verse 1]\nWah-oh-oh-oh-oh\nOh oh oh\nLa la la\nYeah yeah";
        let good = "[Verse 1]\nThe sun is shining down on me\nRiverbeds are lined with stones\n\
                    Gouda gold in farming fields\nWe carved out our own space\n\
                    Feet on the ground, stories told\nThe breeze is whispering secrets";

        assert!(score_lyrics(repetitive) < 0.3, "repetition should score badly");
        assert!(score_lyrics(vocalising) < 0.3, "filler should score badly");
        assert!(score_lyrics(good) >= GOOD_ENOUGH, "real verses should pass");
        assert_eq!(score_lyrics(""), 0.0);
        assert_eq!(score_lyrics("[Intro]\n[Outro]"), 0.0, "markers alone are not a song");
    }

    #[test]
    fn a_couple_of_good_lines_is_not_enough() {
        // Distinct, but too short to be a song — must not beat a full lyric.
        let stub = "[Verse]\nOne good line here\nAnother good line";
        assert!(score_lyrics(stub) < GOOD_ENOUGH);
    }

    #[test]
    fn recognises_filler_without_eating_real_words() {
        assert!(is_filler("oh oh oh"));
        assert!(is_filler("La la la"));
        assert!(is_filler("Wah-oh-oh-oh"));
        assert!(!is_filler("The sun is shining"));
        // "Yeah" alone is filler; used in a sentence it isn't.
        assert!(is_filler("yeah yeah"));
        assert!(!is_filler("yeah I remember"));
    }

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
