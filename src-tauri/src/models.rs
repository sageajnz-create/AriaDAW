//! First-run model download.
//!
//! The models are about 12 GB, so they can't ship inside a package — every
//! install has to fetch them once. This is the first thing a new user meets,
//! so it says what it's doing in plain terms, resumes rather than restarting
//! after a dropped connection, and verifies what it got.
//!
//! Tiers exist because the difference between a 4 GB card and an 8 GB one is
//! the difference between usable and not. Nobody should have to read a table to
//! find that out; we detect VRAM and pick.

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const REPO: &str = "https://huggingface.co/Serveurperso/ACE-Step-1.5-GGUF/resolve/main";

#[derive(Debug, Clone, Serialize)]
pub struct ModelFile {
    pub name: String,
    /// Approximate size in bytes, for progress before the download starts.
    pub bytes: u64,
    pub role: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Runs on ~4 GB of VRAM, or on the CPU. Weakest lyrics.
    Light,
    /// ~6 GB. A good middle.
    Standard,
    /// ~8 GB or more. Measured at 7048 MB peak on an RX 6650 XT.
    Best,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Light => "Light",
            Tier::Standard => "Standard",
            Tier::Best => "Best",
        }
    }

    /// What a person actually needs to know to choose.
    pub fn description(self) -> &'static str {
        match self {
            Tier::Light => {
                "Works on almost any computer, including without a graphics card. \
                            Songs take longer and the words are simpler."
            }
            Tier::Standard => "A good balance. Needs a graphics card with about 6 GB.",
            Tier::Best => {
                "The best words and the fullest sound. Needs a graphics card with \
                           8 GB or more."
            }
        }
    }

    pub fn files(self) -> Vec<ModelFile> {
        let mk = |name: &str, bytes: u64, role: &'static str| ModelFile {
            name: name.to_string(),
            bytes,
            role,
        };
        // The VAE is always full precision: it's small and quality-critical.
        let vae = mk("vae-BF16.gguf", 338_000_000, "decoder");
        let enc = mk(
            "Qwen3-Embedding-0.6B-Q8_0.gguf",
            784_000_000,
            "text encoder",
        );
        match self {
            Tier::Light => vec![
                vae,
                enc,
                mk("acestep-5Hz-lm-0.6B-Q8_0.gguf", 745_000_000, "songwriter"),
                mk("acestep-v15-turbo-Q6_K.gguf", 2_070_000_000, "sound model"),
            ],
            Tier::Standard => vec![
                vae,
                enc,
                mk("acestep-5Hz-lm-1.7B-Q8_0.gguf", 2_040_000_000, "songwriter"),
                mk("acestep-v15-turbo-Q8_0.gguf", 2_680_000_000, "sound model"),
                mk(
                    "acestep-v15-sft-Q8_0.gguf",
                    2_680_000_000,
                    "detailed sound model",
                ),
            ],
            Tier::Best => vec![
                vae,
                enc,
                mk("acestep-5Hz-lm-4B-Q8_0.gguf", 4_680_000_000, "songwriter"),
                mk("acestep-v15-turbo-Q8_0.gguf", 2_680_000_000, "sound model"),
                mk(
                    "acestep-v15-sft-Q8_0.gguf",
                    2_680_000_000,
                    "detailed sound model",
                ),
            ],
        }
    }

    pub fn total_bytes(self) -> u64 {
        self.files().iter().map(|f| f.bytes).sum()
    }
}

/// Pick a tier from the GPU's memory.
///
/// Reads the amdgpu sysfs node, which is exact where it exists. Everything else
/// falls back to Standard rather than guessing high — a download that turns out
/// not to run is a worse first experience than one that is merely not the best
/// available.
pub fn suggested_tier() -> Tier {
    if let Some(mb) = detect_vram_mb() {
        return match mb {
            m if m >= 7800 => Tier::Best,
            m if m >= 5600 => Tier::Standard,
            _ => Tier::Light,
        };
    }
    Tier::Standard
}

pub fn detect_vram_mb() -> Option<u64> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    let mut best = 0u64;
    for e in entries.flatten() {
        let p = e.path().join("device/mem_info_vram_total");
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Ok(bytes) = text.trim().parse::<u64>() {
                best = best.max(bytes / 1024 / 1024);
            }
        }
    }
    (best > 0).then_some(best)
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub file: String,
    pub role: &'static str,
    pub file_index: usize,
    pub file_count: usize,
    pub downloaded: u64,
    pub total: u64,
}

/// Download every file for a tier that isn't already present.
///
/// Resumes with a Range request when a partial file is found: these are
/// multi-gigabyte downloads and starting over because a laptop slept is not
/// acceptable.
pub fn download_tier(
    dir: &Path,
    tier: Tier,
    mut on_progress: impl FnMut(DownloadProgress),
    should_stop: impl Fn() -> bool,
) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let files = tier.files();
    let count = files.len();

    let client = reqwest::blocking::Client::builder().timeout(None).build()?;

    for (index, file) in files.iter().enumerate() {
        let dest = dir.join(&file.name);
        if is_complete(&dest, &client, &file.name)? {
            on_progress(DownloadProgress {
                file: file.name.clone(),
                role: file.role,
                file_index: index,
                file_count: count,
                downloaded: file.bytes,
                total: file.bytes,
            });
            continue;
        }

        let partial = dest.with_extension("part");
        let have = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);

        let mut req = client.get(format!("{REPO}/{}", file.name));
        if have > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let mut resp = req
            .send()
            .with_context(|| format!("downloading {}", file.name))?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "could not download {} ({})",
                file.name,
                resp.status()
            ));
        }

        let total = resp
            .content_length()
            .map(|l| l + have)
            .unwrap_or(file.bytes);

        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .append(have > 0)
            .write(true)
            .open(&partial)?;

        let mut downloaded = have;
        let mut buf = vec![0u8; 1 << 20];
        let mut since_report = 0u64;
        loop {
            if should_stop() {
                return Err(anyhow!("cancelled"));
            }
            let n = resp.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            downloaded += n as u64;
            since_report += n as u64;
            // Report every few megabytes rather than every chunk; the UI does
            // not need a thousand events a second.
            if since_report > 4 << 20 {
                since_report = 0;
                on_progress(DownloadProgress {
                    file: file.name.clone(),
                    role: file.role,
                    file_index: index,
                    file_count: count,
                    downloaded,
                    total,
                });
            }
        }
        out.flush()?;
        drop(out);

        verify_gguf(&partial)
            .with_context(|| format!("{} downloaded but looks damaged", file.name))?;
        std::fs::rename(&partial, &dest)?;

        on_progress(DownloadProgress {
            file: file.name.clone(),
            role: file.role,
            file_index: index,
            file_count: count,
            downloaded: total,
            total,
        });
    }
    Ok(())
}

/// A file counts as present only if it's whole. A truncated model produces a
/// baffling failure much later, so the size is checked against the server.
fn is_complete(path: &Path, client: &reqwest::blocking::Client, name: &str) -> Result<bool> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Ok(false);
    };
    if verify_gguf(path).is_err() {
        return Ok(false);
    }
    let remote = client
        .head(format!("{REPO}/{name}"))
        .send()
        .ok()
        .and_then(|r| r.content_length());
    Ok(match remote {
        Some(len) => meta.len() == len,
        // Offline, but the file is on disk and has a valid header: use it.
        None => true,
    })
}

/// GGUF files start with the magic "GGUF". Cheap guard against a half-written
/// file or an HTML error page saved under a .gguf name.
fn verify_gguf(path: &Path) -> Result<()> {
    let mut f = std::fs::File::open(path)?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        return Err(anyhow!("not a model file"));
    }
    Ok(())
}

/// Which files a tier is still missing.
pub fn missing_files(dir: &std::path::Path, tier: Tier) -> Vec<ModelFile> {
    tier.files()
        .into_iter()
        .filter(|f| {
            let p = dir.join(&f.name);
            !p.exists() || verify_gguf(&p).is_err()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tier_has_the_four_essential_parts() {
        for tier in [Tier::Light, Tier::Standard, Tier::Best] {
            let files = tier.files();
            for role in ["decoder", "text encoder", "songwriter", "sound model"] {
                assert!(
                    files.iter().any(|f| f.role == role),
                    "{} tier is missing its {role}",
                    tier.label()
                );
            }
            assert!(tier.total_bytes() > 3_000_000_000);
        }
    }

    #[test]
    fn tiers_grow_with_capability() {
        assert!(Tier::Light.total_bytes() < Tier::Standard.total_bytes());
        assert!(Tier::Standard.total_bytes() < Tier::Best.total_bytes());
    }

    #[test]
    fn stem_work_needs_the_detailed_model() {
        // extract/lego/complete require the SFT model; the Light tier can't do
        // them and the UI must not offer them there.
        assert!(!Tier::Light.files().iter().any(|f| f.name.contains("sft")));
        assert!(Tier::Standard
            .files()
            .iter()
            .any(|f| f.name.contains("sft")));
    }
}
