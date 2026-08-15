//! Operations that make a new track from one you already have.
//!
//! All of these are native to ACE-Step, so they need no extra models or
//! dependencies beyond the SFT diffusion weights. Notably, Suno gates stem
//! separation and section editing behind paid tiers; here they are simply part
//! of the app, and the results are yours like everything else.
//!
//! Capability notes measured against the engine docs:
//! - `cover` and `repaint` work with the fast turbo model.
//! - `extract` (stems) and `lego` (add an instrument) require the SFT model.
//! - "Extend" is not a task type. It is `repaint` with the painted region
//!   pushed past the end of the source — outpainting. `complete` sounds like
//!   extend but is not: it builds a full mix from an isolated stem.

use serde::{Deserialize, Serialize};

/// Stems the model can isolate. ACE-Step supports twelve, well past the four
/// that Demucs offers, which is why Aria needs no separate stem tool.
pub const STEMS: &[(&str, &str)] = &[
    ("vocals", "Vocals"),
    ("backing_vocals", "Backing vocals"),
    ("drums", "Drums"),
    ("bass", "Bass"),
    ("guitar", "Guitar"),
    ("keyboard", "Keyboard"),
    ("percussion", "Percussion"),
    ("strings", "Strings"),
    ("synth", "Synth"),
    ("brass", "Brass"),
    ("woodwinds", "Woodwinds"),
    ("fx", "Effects"),
];

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    /// Isolate one instrument or voice from the mix.
    Stem { track: String },
    /// Restyle the whole song, keeping its structure.
    Cover { caption: String, strength: f64 },
    /// Regenerate a slice of the song between two times, in seconds.
    Repaint { start: f64, end: f64, caption: Option<String> },
    /// Make the song longer by painting past its end.
    Extend { seconds: f64 },
    /// Add a named instrument layer over the existing audio.
    AddLayer { track: String, caption: Option<String> },
}

impl Operation {
    /// Short label stored on the derived track, for the library's lineage view.
    pub fn label(&self) -> String {
        match self {
            Operation::Stem { track } => format!("stem: {track}"),
            Operation::Cover { .. } => "cover".into(),
            Operation::Repaint { start, end, .. } => format!("repaint {start:.0}-{end:.0}s"),
            Operation::Extend { seconds } => format!("extend +{seconds:.0}s"),
            Operation::AddLayer { track, .. } => format!("added {track}"),
        }
    }

    /// How the new track should be titled, given the source's title.
    pub fn title_for(&self, source: &str) -> String {
        match self {
            Operation::Stem { track } => {
                let pretty = STEMS
                    .iter()
                    .find(|(k, _)| k == track)
                    .map(|(_, n)| *n)
                    .unwrap_or(track.as_str());
                format!("{source} — {pretty}")
            }
            Operation::Cover { .. } => format!("{source} (cover)"),
            Operation::Repaint { .. } => format!("{source} (edited)"),
            Operation::Extend { .. } => format!("{source} (longer)"),
            Operation::AddLayer { track, .. } => format!("{source} + {track}"),
        }
    }

    /// These need the SFT/base diffusion model; turbo cannot run them.
    pub fn needs_sft(&self) -> bool {
        matches!(self, Operation::Stem { .. } | Operation::AddLayer { .. })
    }

    pub fn task_type(&self) -> &'static str {
        match self {
            Operation::Stem { .. } => "extract",
            Operation::Cover { .. } => "cover",
            Operation::Repaint { .. } | Operation::Extend { .. } => "repaint",
            Operation::AddLayer { .. } => "lego",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StemChoice {
    pub id: String,
    pub name: String,
}

pub fn stem_choices() -> Vec<StemChoice> {
    STEMS
        .iter()
        .map(|(id, name)| StemChoice { id: (*id).into(), name: (*name).into() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_describe_the_derivation() {
        let s = Operation::Stem { track: "drums".into() };
        assert_eq!(s.title_for("My Song"), "My Song — Drums");
        assert_eq!(s.task_type(), "extract");
        assert!(s.needs_sft());

        let e = Operation::Extend { seconds: 30.0 };
        assert_eq!(e.title_for("My Song"), "My Song (longer)");
        // Extend is outpainting, not its own task type.
        assert_eq!(e.task_type(), "repaint");
        assert!(!e.needs_sft());
    }

    #[test]
    fn labels_carry_enough_to_reconstruct_lineage() {
        assert_eq!(Operation::Stem { track: "bass".into() }.label(), "stem: bass");
        assert_eq!(
            Operation::Repaint { start: 10.0, end: 20.0, caption: None }.label(),
            "repaint 10-20s"
        );
    }
}
