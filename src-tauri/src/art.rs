//! Cover art.
//!
//! Every song gets a picture. Suno gives each track AI-generated artwork, and
//! without something in that slot a library reads as a spreadsheet of filenames
//! rather than a shelf of music — you stop recognising your own songs.
//!
//! We do not have an image model and are not going to ship a second multi-GB
//! download for decoration. Instead the art is drawn: a deterministic
//! composition derived from the track's id, so a song's cover is stable
//! forever, identical on every machine, and costs no GPU time.
//!
//! Output is SVG for three reasons: it needs no encoder dependency, it stays
//! sharp at any size, and it is an ordinary file the user can open, convert or
//! use as they like — the same promise the audio makes.

use std::path::{Path, PathBuf};

/// Square edge of the artwork's coordinate space. SVG scales, so this is only
/// the unit the composition is written in.
const SIZE: f64 = 640.0;

/// A cover for `id`, as a standalone SVG document.
pub fn svg_for(id: &str) -> String {
    let h = hash64(id);
    let mut rng = Rng::new(h);
    // Ids are scoped per document, but suffixing keeps the markup safe to
    // inline into a page alongside other covers later.
    let tag = format!("{:x}", h & 0xffff_ffff);

    // One hue anchors the whole cover; the accent sits a deliberate distance
    // away so the two never muddy into each other.
    let hue = rng.range(0.0, 360.0);
    let accent = (hue + rng.range(100.0, 200.0)) % 360.0;

    let deep = hsl(hue, rng.range(0.45, 0.7), rng.range(0.10, 0.17));
    let mid = hsl((hue + 20.0) % 360.0, rng.range(0.5, 0.75), rng.range(0.22, 0.32));
    let glow = hsl(accent, rng.range(0.6, 0.85), rng.range(0.55, 0.68));
    let hilite = hsl((accent + 30.0) % 360.0, rng.range(0.55, 0.8), rng.range(0.62, 0.75));

    let angle = rng.range(0.0, 360.0).to_radians();
    let (x1, y1, x2, y2) = (
        50.0 - 50.0 * angle.cos(),
        50.0 - 50.0 * angle.sin(),
        50.0 + 50.0 * angle.cos(),
        50.0 + 50.0 * angle.sin(),
    );

    let mut svg = String::with_capacity(4096);
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE:.0} {SIZE:.0}" width="{SIZE:.0}" height="{SIZE:.0}" role="img" aria-label="Cover art">"#
    ));
    svg.push_str("<defs>");
    svg.push_str(&format!(
        r#"<linearGradient id="bg{tag}" x1="{x1:.1}%" y1="{y1:.1}%" x2="{x2:.1}%" y2="{y2:.1}%"><stop offset="0" stop-color="{deep}"/><stop offset="1" stop-color="{mid}"/></linearGradient>"#
    ));
    svg.push_str(&format!(
        r#"<filter id="soft{tag}" x="-40%" y="-40%" width="180%" height="180%"><feGaussianBlur stdDeviation="70"/></filter>"#
    ));
    // Grain keeps large flat gradients from banding, and reads as print texture.
    svg.push_str(&format!(
        r#"<filter id="grain{tag}"><feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="3" seed="{}"/><feColorMatrix type="saturate" values="0"/></filter>"#,
        h % 9973
    ));
    svg.push_str(&format!(
        // Doubled hashes: `="#` would otherwise close a plain r#"" string.
        r##"<radialGradient id="vig{tag}"><stop offset="0.55" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity="0.5"/></radialGradient>"##
    ));
    svg.push_str("</defs>");

    svg.push_str(&format!(
        r#"<rect width="{SIZE:.0}" height="{SIZE:.0}" fill="url(#bg{tag})"/>"#
    ));

    // Soft colour fields. Blurred well past their own radius, they behave like
    // a gradient mesh without needing one.
    svg.push_str(&format!(r#"<g filter="url(#soft{tag})" opacity="0.9">"#));
    for i in 0..3 {
        let cx = rng.range(80.0, SIZE - 80.0);
        let cy = rng.range(60.0, SIZE - 140.0);
        let r = rng.range(110.0, 240.0);
        let fill = if i == 0 { &glow } else if i == 1 { &hilite } else { &mid };
        let op = rng.range(0.35, 0.7);
        svg.push_str(&format!(
            r#"<circle cx="{cx:.0}" cy="{cy:.0}" r="{r:.0}" fill="{fill}" opacity="{op:.2}"/>"#
        ));
    }
    svg.push_str("</g>");

    // Two motifs, so a library of covers doesn't look stamped from one mould.
    if h & 1 == 0 {
        svg.push_str(&bars(&mut rng, &hilite, &glow));
    } else {
        svg.push_str(&rings(&mut rng, &hilite, &glow));
    }

    svg.push_str(&format!(
        r#"<rect width="{SIZE:.0}" height="{SIZE:.0}" fill="url(#vig{tag})"/>"#
    ));
    svg.push_str(&format!(
        r#"<rect width="{SIZE:.0}" height="{SIZE:.0}" filter="url(#grain{tag})" opacity="0.13"/>"#
    ));
    svg.push_str("</svg>");
    svg
}

/// A spectrum-like row of rounded columns.
fn bars(rng: &mut Rng, near: &str, far: &str) -> String {
    let count = 14 + rng.pick(12);
    let gap = 6.0;
    let span = SIZE - 120.0;
    let w = (span - gap * (count - 1) as f64) / count as f64;
    let base = SIZE - 96.0;
    let mut out = String::from(r#"<g opacity="0.92">"#);
    for i in 0..count {
        let x = 60.0 + i as f64 * (w + gap);
        // A gentle arc under the random heights keeps it from looking like pure
        // noise — the eye reads a phrase rather than a bar chart.
        let t = i as f64 / (count - 1).max(1) as f64;
        let arc = (t * std::f64::consts::PI).sin();
        let hgt = (rng.range(0.15, 1.0) * 0.55 + arc * 0.45) * 250.0 + 18.0;
        let fill = if i % 3 == 0 { far } else { near };
        let op = rng.range(0.55, 1.0);
        out.push_str(&format!(
            r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{hgt:.1}" rx="{r:.1}" fill="{fill}" opacity="{op:.2}"/>"#,
            y = base - hgt,
            r = (w / 2.0).min(9.0),
        ));
    }
    out.push_str("</g>");
    out
}

/// Concentric arcs, offset from centre so the composition stays asymmetric.
fn rings(rng: &mut Rng, near: &str, far: &str) -> String {
    let cx = rng.range(200.0, 440.0);
    let cy = rng.range(200.0, 440.0);
    let count = 5 + rng.pick(6);
    let mut out = String::from(r#"<g fill="none" stroke-linecap="round">"#);
    let mut r = rng.range(38.0, 70.0);
    for i in 0..count {
        let sw = rng.range(2.0, 14.0);
        let stroke = if i % 2 == 0 { near } else { far };
        let op = rng.range(0.35, 0.9);
        // Broken rings: a full circle every few steps, arcs otherwise.
        if i % 3 == 2 {
            out.push_str(&format!(
                r#"<circle cx="{cx:.0}" cy="{cy:.0}" r="{r:.0}" stroke="{stroke}" stroke-width="{sw:.1}" opacity="{op:.2}"/>"#
            ));
        } else {
            let start = rng.range(0.0, std::f64::consts::TAU);
            let sweep = rng.range(1.2, 5.0);
            let (x0, y0) = (cx + r * start.cos(), cy + r * start.sin());
            let (x1, y1) = (cx + r * (start + sweep).cos(), cy + r * (start + sweep).sin());
            let large = if sweep > std::f64::consts::PI { 1 } else { 0 };
            out.push_str(&format!(
                r#"<path d="M {x0:.1} {y0:.1} A {r:.1} {r:.1} 0 {large} 1 {x1:.1} {y1:.1}" stroke="{stroke}" stroke-width="{sw:.1}" opacity="{op:.2}"/>"#
            ));
        }
        r += rng.range(22.0, 52.0);
    }
    out.push_str("</g>");
    out
}

/// Where a track's cover lives: beside its audio, same stem.
pub fn path_beside(audio_path: &str) -> PathBuf {
    Path::new(audio_path).with_extension("svg")
}

/// Write the cover next to the audio, unless it's already there.
///
/// Best-effort on purpose. Art is decoration; a read-only music folder or a
/// full disk must never be able to stop a song from playing.
pub fn save_beside(audio_path: &str, svg: &str) {
    if audio_path.is_empty() {
        return;
    }
    let target = path_beside(audio_path);
    if target.exists() {
        return;
    }
    // Only alongside audio that actually exists — otherwise a moved file would
    // leave orphan artwork scattered around.
    if !Path::new(audio_path).exists() {
        return;
    }
    if let Err(e) = std::fs::write(&target, svg) {
        eprintln!("[art] could not write {}: {e}", target.display());
    }
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // xorshift is fixed at zero, and any non-zero start is fine.
        Rng(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.unit() * (hi - lo)
    }
    fn pick(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// FNV-1a. Not cryptographic — it only has to scatter ids evenly and never
/// change, so today's covers still look like themselves next year.
fn hash64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h
}

fn hsl(h: f64, s: f64, l: f64) -> String {
    let h = ((h % 360.0) + 360.0) % 360.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f64| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", to(r), to(g), to(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_track_always_gets_the_same_cover() {
        assert_eq!(svg_for("track-abc"), svg_for("track-abc"));
        assert_ne!(svg_for("track-abc"), svg_for("track-abd"));
    }

    #[test]
    fn output_is_a_self_contained_svg_document() {
        let svg = svg_for("6f1c2d9e-0000-4000-8000-000000000000");
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.ends_with("</svg>"));
        // No external references: covers must render with the network off.
        assert!(!svg.contains("http://") || !svg.contains("xlink:href"));
        assert_eq!(svg.matches("<svg").count(), 1);
    }

    #[test]
    fn both_motifs_are_reachable() {
        let mut bars = false;
        let mut rings = false;
        for i in 0..64 {
            let svg = svg_for(&format!("track-{i}"));
            bars |= svg.contains("<rect x=");
            rings |= svg.contains("stroke-linecap");
        }
        assert!(bars && rings, "expected both compositions across 64 ids");
    }

    #[test]
    fn hsl_maps_to_the_hex_a_browser_would_paint() {
        assert_eq!(hsl(0.0, 1.0, 0.5), "#ff0000");
        assert_eq!(hsl(120.0, 1.0, 0.5), "#00ff00");
        assert_eq!(hsl(240.0, 1.0, 0.5), "#0000ff");
        assert_eq!(hsl(0.0, 0.0, 0.0), "#000000");
        assert_eq!(hsl(0.0, 0.0, 1.0), "#ffffff");
    }

    #[test]
    fn art_sits_beside_the_audio() {
        assert_eq!(
            path_beside("/home/u/Music/Aria/song.mp3"),
            PathBuf::from("/home/u/Music/Aria/song.svg")
        );
    }
}
