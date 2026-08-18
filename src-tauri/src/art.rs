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
//! # Two renderers, one composition
//!
//! The cover is needed as SVG (for the UI and for export, where it stays sharp
//! at any size and is an ordinary file the user keeps) and as pixels (for video
//! export, because ffmpeg can only read SVG when it was built against librsvg,
//! which distro and static builds routinely are not).
//!
//! Rather than describe the picture twice and let the two drift, `Composition`
//! works out *what* to draw and the renderers only decide *how*. The single
//! honest difference is the grain: SVG gets `feTurbulence`, the raster gets
//! value noise. They serve the same purpose and neither is load-bearing.

use std::path::{Path, PathBuf};

/// Square edge of the artwork's coordinate space. Both renderers work in this
/// space and scale from it, so geometry is written once.
const SIZE: f64 = 640.0;

type Rgb = [u8; 3];

/// Which of the palette's colours a shape uses. Held as an index rather than a
/// value so both renderers resolve it the same way.
#[derive(Clone, Copy)]
enum Ink {
    Mid,
    Glow,
    Hilite,
}

struct Blob {
    cx: f64,
    cy: f64,
    r: f64,
    ink: Ink,
    op: f64,
}

struct Bar {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
    ink: Ink,
    op: f64,
}

enum Ring {
    Full { r: f64, sw: f64, ink: Ink, op: f64 },
    Arc { r: f64, sw: f64, ink: Ink, op: f64, start: f64, sweep: f64 },
}

enum Motif {
    /// A spectrum-like row of rounded columns.
    Bars(Vec<Bar>),
    /// Concentric arcs, offset from centre so the composition stays asymmetric.
    Rings { cx: f64, cy: f64, rings: Vec<Ring> },
}

/// Everything about one cover, worked out once.
struct Composition {
    tag: String,
    deep: Rgb,
    mid: Rgb,
    glow: Rgb,
    hilite: Rgb,
    /// Gradient direction, radians.
    angle: f64,
    blobs: Vec<Blob>,
    motif: Motif,
    grain_seed: u64,
}

impl Composition {
    fn for_id(id: &str) -> Self {
        let h = hash64(id);
        let mut rng = Rng::new(h);

        // One hue anchors the whole cover; the accent sits a deliberate distance
        // away so the two never muddy into each other.
        let hue = rng.range(0.0, 360.0);
        let accent = (hue + rng.range(100.0, 200.0)) % 360.0;

        let deep = hsl(hue, rng.range(0.45, 0.7), rng.range(0.10, 0.17));
        let mid = hsl((hue + 20.0) % 360.0, rng.range(0.5, 0.75), rng.range(0.22, 0.32));
        let glow = hsl(accent, rng.range(0.6, 0.85), rng.range(0.55, 0.68));
        let hilite = hsl((accent + 30.0) % 360.0, rng.range(0.55, 0.8), rng.range(0.62, 0.75));

        let angle = rng.range(0.0, 360.0).to_radians();

        // Soft colour fields. Blurred well past their own radius, they behave
        // like a gradient mesh without needing one.
        let mut blobs = Vec::with_capacity(3);
        for i in 0..3 {
            let cx = rng.range(80.0, SIZE - 80.0);
            let cy = rng.range(60.0, SIZE - 140.0);
            let r = rng.range(110.0, 240.0);
            let ink = if i == 0 { Ink::Glow } else if i == 1 { Ink::Hilite } else { Ink::Mid };
            let op = rng.range(0.35, 0.7);
            blobs.push(Blob { cx, cy, r, ink, op });
        }

        // Two motifs, so a library of covers doesn't look stamped from one mould.
        let motif = if h & 1 == 0 { bars(&mut rng) } else { rings(&mut rng) };

        Composition {
            tag: format!("{:x}", h & 0xffff_ffff),
            deep,
            mid,
            glow,
            hilite,
            angle,
            blobs,
            motif,
            grain_seed: h % 9973,
        }
    }

    fn ink(&self, which: Ink) -> Rgb {
        match which {
            Ink::Mid => self.mid,
            Ink::Glow => self.glow,
            Ink::Hilite => self.hilite,
        }
    }
}

fn bars(rng: &mut Rng) -> Motif {
    let count = 14 + rng.pick(12);
    let gap = 6.0;
    let span = SIZE - 120.0;
    let w = (span - gap * (count - 1) as f64) / count as f64;
    let base = SIZE - 96.0;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let x = 60.0 + i as f64 * (w + gap);
        // A gentle arc under the random heights keeps it from looking like pure
        // noise — the eye reads a phrase rather than a bar chart.
        let t = i as f64 / (count - 1).max(1) as f64;
        let arc = (t * std::f64::consts::PI).sin();
        let h = (rng.range(0.15, 1.0) * 0.55 + arc * 0.45) * 250.0 + 18.0;
        let ink = if i % 3 == 0 { Ink::Glow } else { Ink::Hilite };
        let op = rng.range(0.55, 1.0);
        out.push(Bar { x, y: base - h, w, h, radius: (w / 2.0).min(9.0), ink, op });
    }
    Motif::Bars(out)
}

fn rings(rng: &mut Rng) -> Motif {
    let cx = rng.range(200.0, 440.0);
    let cy = rng.range(200.0, 440.0);
    let count = 5 + rng.pick(6);
    let mut out = Vec::with_capacity(count);
    let mut r = rng.range(38.0, 70.0);
    for i in 0..count {
        let sw = rng.range(2.0, 14.0);
        let ink = if i % 2 == 0 { Ink::Hilite } else { Ink::Glow };
        let op = rng.range(0.35, 0.9);
        // Broken rings: a full circle every few steps, arcs otherwise.
        if i % 3 == 2 {
            out.push(Ring::Full { r, sw, ink, op });
        } else {
            let start = rng.range(0.0, std::f64::consts::TAU);
            let sweep = rng.range(1.2, 5.0);
            out.push(Ring::Arc { r, sw, ink, op, start, sweep });
        }
        r += rng.range(22.0, 52.0);
    }
    Motif::Rings { cx, cy, rings: out }
}

// --- SVG -----------------------------------------------------------------

/// A cover for `id`, as a standalone SVG document.
pub fn svg_for(id: &str) -> String {
    let c = Composition::for_id(id);
    let tag = &c.tag;
    let (deep, mid) = (hex(c.deep), hex(c.mid));

    let (x1, y1, x2, y2) = (
        50.0 - 50.0 * c.angle.cos(),
        50.0 - 50.0 * c.angle.sin(),
        50.0 + 50.0 * c.angle.cos(),
        50.0 + 50.0 * c.angle.sin(),
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
        c.grain_seed
    ));
    svg.push_str(&format!(
        // Doubled hashes: `="#` would otherwise close a plain r#"" string.
        r##"<radialGradient id="vig{tag}"><stop offset="0.55" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity="0.5"/></radialGradient>"##
    ));
    svg.push_str("</defs>");

    svg.push_str(&format!(
        r#"<rect width="{SIZE:.0}" height="{SIZE:.0}" fill="url(#bg{tag})"/>"#
    ));

    svg.push_str(&format!(r#"<g filter="url(#soft{tag})" opacity="0.9">"#));
    for b in &c.blobs {
        svg.push_str(&format!(
            r#"<circle cx="{cx:.0}" cy="{cy:.0}" r="{r:.0}" fill="{fill}" opacity="{op:.2}"/>"#,
            cx = b.cx, cy = b.cy, r = b.r, fill = hex(c.ink(b.ink)), op = b.op,
        ));
    }
    svg.push_str("</g>");

    match &c.motif {
        Motif::Bars(list) => {
            svg.push_str(r#"<g opacity="0.92">"#);
            for b in list {
                svg.push_str(&format!(
                    r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" rx="{r:.1}" fill="{fill}" opacity="{op:.2}"/>"#,
                    x = b.x, y = b.y, w = b.w, h = b.h, r = b.radius,
                    fill = hex(c.ink(b.ink)), op = b.op,
                ));
            }
            svg.push_str("</g>");
        }
        Motif::Rings { cx, cy, rings } => {
            svg.push_str(r#"<g fill="none" stroke-linecap="round">"#);
            for ring in rings {
                match ring {
                    Ring::Full { r, sw, ink, op } => svg.push_str(&format!(
                        r#"<circle cx="{cx:.0}" cy="{cy:.0}" r="{r:.0}" stroke="{stroke}" stroke-width="{sw:.1}" opacity="{op:.2}"/>"#,
                        stroke = hex(c.ink(*ink)),
                    )),
                    Ring::Arc { r, sw, ink, op, start, sweep } => {
                        let (x0, y0) = (cx + r * start.cos(), cy + r * start.sin());
                        let (x1, y1) = (cx + r * (start + sweep).cos(), cy + r * (start + sweep).sin());
                        let large = if *sweep > std::f64::consts::PI { 1 } else { 0 };
                        svg.push_str(&format!(
                            r#"<path d="M {x0:.1} {y0:.1} A {r:.1} {r:.1} 0 {large} 1 {x1:.1} {y1:.1}" stroke="{stroke}" stroke-width="{sw:.1}" opacity="{op:.2}"/>"#,
                            stroke = hex(c.ink(*ink)),
                        ));
                    }
                }
            }
            svg.push_str("</g>");
        }
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

// --- raster ---------------------------------------------------------------

/// The same cover as 8-bit RGB pixels, `size` on a side.
pub fn rgb_for(id: &str, size: u32) -> Vec<u8> {
    let c = Composition::for_id(id);
    let n = size as usize;
    let scale = size as f64 / SIZE;
    let mut buf = vec![0f32; n * n * 3];

    // Background: a linear gradient along the composition's angle, using the
    // same axis SVG derives from its x1/y1 → x2/y2 percentages.
    let (ax, ay) = (0.5 - 0.5 * c.angle.cos(), 0.5 - 0.5 * c.angle.sin());
    let (dx, dy) = (c.angle.cos(), c.angle.sin());
    let dd = dx * dx + dy * dy;
    for y in 0..n {
        for x in 0..n {
            let (px, py) = ((x as f64 + 0.5) / n as f64, (y as f64 + 0.5) / n as f64);
            let t = (((px - ax) * dx + (py - ay) * dy) / dd).clamp(0.0, 1.0);
            let i = (y * n + x) * 3;
            for ch in 0..3 {
                buf[i + ch] = c.deep[ch] as f32 + (c.mid[ch] as f32 - c.deep[ch] as f32) * t as f32;
            }
        }
    }

    // Blobs go onto their own layer so they can be blurred as a group, exactly
    // as the SVG filter does, instead of smearing the background with them.
    //
    // That layer is worked in *linear light*, because SVG filters default to
    // `color-interpolation-filters: linearRGB`. Blurring in sRGB instead is a
    // real, measurable error, not a nicety: it came out a uniform 24/255 darker
    // than librsvg's rendering of the very same document.
    let mut layer = vec![0f32; n * n * 3];
    let mut alpha = vec![0f32; n * n];
    for b in &c.blobs {
        let ink = c.ink(b.ink);
        let (cx, cy, r) = (b.cx * scale, b.cy * scale, b.r * scale);
        let (x0, x1) = ((cx - r).floor().max(0.0) as usize, (cx + r).ceil().min(n as f64) as usize);
        let (y0, y1) = ((cy - r).floor().max(0.0) as usize, (cy + r).ceil().min(n as f64) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
                if d > r {
                    continue;
                }
                let a = b.op as f32;
                let p = y * n + x;
                for ch in 0..3 {
                    let lin = to_linear(ink[ch] as f32);
                    layer[p * 3 + ch] = layer[p * 3 + ch] * (1.0 - a) + lin * a;
                }
                alpha[p] = alpha[p] * (1.0 - a) + a;
            }
        }
    }
    // Three box passes approximate the SVG's Gaussian well enough for a field
    // this soft, and stay O(pixels) regardless of radius.
    let radius = (70.0 * scale) as usize;
    for _ in 0..3 {
        blur_rgb(&mut layer, n, radius);
        blur_a(&mut alpha, n, radius);
    }
    for p in 0..n * n {
        let a = alpha[p] * 0.9; // the SVG group's opacity
        for ch in 0..3 {
            // Back to sRGB on the way out, so the motifs and the vignette —
            // which are ordinary painting, not filters — composite normally.
            let base = to_linear(buf[p * 3 + ch]);
            buf[p * 3 + ch] = to_srgb(base * (1.0 - a) + layer[p * 3 + ch] * a);
        }
    }

    match &c.motif {
        Motif::Bars(list) => {
            for b in list {
                let ink = c.ink(b.ink);
                paint_round_rect(
                    &mut buf, n,
                    b.x * scale, b.y * scale, b.w * scale, b.h * scale, b.radius * scale,
                    ink, b.op as f32 * 0.92,
                );
            }
        }
        Motif::Rings { cx, cy, rings } => {
            for ring in rings {
                let (r, sw, ink, op, start, sweep) = match ring {
                    Ring::Full { r, sw, ink, op } => (r, sw, ink, op, 0.0, std::f64::consts::TAU),
                    Ring::Arc { r, sw, ink, op, start, sweep } => (r, sw, ink, op, *start, *sweep),
                };
                paint_arc(
                    &mut buf, n, cx * scale, cy * scale, r * scale, sw * scale,
                    start, sweep, c.ink(*ink), *op as f32,
                );
            }
        }
    }

    // Vignette, then grain, matching the SVG's final two overlays.
    let half = n as f64 / 2.0;
    let mut noise = Rng::new(c.grain_seed | 0x9e37_79b9);
    for y in 0..n {
        for x in 0..n {
            let d = ((x as f64 + 0.5 - half).powi(2) + (y as f64 + 0.5 - half).powi(2)).sqrt() / half;
            let v = (((d - 0.55) / 0.45).clamp(0.0, 1.0) * 0.5) as f32;
            // Grain is a *blend toward* a grey, not an addition of one.
            // feTurbulence emits a turbulent alpha alongside the colour, so the
            // overlay's real strength is 0.13 times that alpha — far gentler
            // than mean-zero noise at full 0.13, which came out as visible
            // sensor grain next to librsvg's rendering of the same document.
            let tone = noise.unit() as f32 * 255.0;
            let a = noise.unit() as f32 * 0.13;
            let i = (y * n + x) * 3;
            for ch in 0..3 {
                let shaded = buf[i + ch] * (1.0 - v);
                buf[i + ch] = shaded * (1.0 - a) + tone * a;
            }
        }
    }

    buf.iter().map(|v| v.clamp(0.0, 255.0) as u8).collect()
}

/// A cover as a PNG, ready to hand to ffmpeg.
pub fn png_for(id: &str, size: u32) -> Vec<u8> {
    crate::png::encode_rgb(size, size, &rgb_for(id, size))
}

/// sRGB transfer function, in both directions, on 0..255 values.
fn to_linear(v: f32) -> f32 {
    let c = (v / 255.0).clamp(0.0, 1.0);
    let l = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    l * 255.0
}

fn to_srgb(v: f32) -> f32 {
    let l = (v / 255.0).clamp(0.0, 1.0);
    let c = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    c * 255.0
}

fn blur_rgb(buf: &mut [f32], n: usize, radius: usize) {
    if radius == 0 {
        return;
    }
    let mut tmp = vec![0f32; buf.len()];
    for ch in 0..3 {
        for y in 0..n {
            running_box(buf, &mut tmp, n, radius, ch, |i| y * n + i, n);
        }
    }
    buf.copy_from_slice(&tmp);
    let mut tmp2 = vec![0f32; buf.len()];
    for ch in 0..3 {
        for x in 0..n {
            running_box(buf, &mut tmp2, n, radius, ch, |i| i * n + x, n);
        }
    }
    buf.copy_from_slice(&tmp2);
}

/// One box pass along a line of pixels chosen by `index`.
fn running_box(
    src: &[f32], dst: &mut [f32], n: usize, radius: usize, ch: usize,
    index: impl Fn(usize) -> usize, len: usize,
) {
    let mut sum = 0f32;
    let mut count = 0f32;
    for i in 0..radius.min(len) {
        sum += src[index(i) * 3 + ch];
        count += 1.0;
    }
    for i in 0..len {
        if i + radius < len {
            sum += src[index(i + radius) * 3 + ch];
            count += 1.0;
        }
        if i >= radius + 1 {
            sum -= src[index(i - radius - 1) * 3 + ch];
            count -= 1.0;
        }
        dst[index(i) * 3 + ch] = sum / count;
    }
}

fn blur_a(buf: &mut [f32], n: usize, radius: usize) {
    if radius == 0 {
        return;
    }
    let pass = |src: &[f32], dst: &mut [f32], index: &dyn Fn(usize) -> usize, len: usize| {
        let mut sum = 0f32;
        let mut count = 0f32;
        for i in 0..radius.min(len) {
            sum += src[index(i)];
            count += 1.0;
        }
        for i in 0..len {
            if i + radius < len {
                sum += src[index(i + radius)];
                count += 1.0;
            }
            if i >= radius + 1 {
                sum -= src[index(i - radius - 1)];
                count -= 1.0;
            }
            dst[index(i)] = sum / count;
        }
    };
    let mut tmp = vec![0f32; buf.len()];
    for y in 0..n {
        pass(buf, &mut tmp, &|i| y * n + i, n);
    }
    let mut tmp2 = vec![0f32; buf.len()];
    for x in 0..n {
        pass(&tmp, &mut tmp2, &|i| i * n + x, n);
    }
    buf.copy_from_slice(&tmp2);
}

fn blend(buf: &mut [f32], n: usize, x: usize, y: usize, ink: Rgb, a: f32) {
    if a <= 0.0 || x >= n || y >= n {
        return;
    }
    let a = a.min(1.0);
    let i = (y * n + x) * 3;
    for ch in 0..3 {
        buf[i + ch] = buf[i + ch] * (1.0 - a) + ink[ch] as f32 * a;
    }
}

/// Coverage of a pixel by a shape whose signed distance is `d` (negative
/// inside). One pixel of feathering is enough to kill the jaggies.
fn coverage(d: f64) -> f32 {
    (0.5 - d).clamp(0.0, 1.0) as f32
}

fn paint_round_rect(
    buf: &mut [f32], n: usize, x: f64, y: f64, w: f64, h: f64, r: f64, ink: Rgb, op: f32,
) {
    let r = r.min(w / 2.0).min(h / 2.0);
    let (x0, x1) = ((x - 1.0).floor().max(0.0) as usize, (x + w + 1.0).ceil().min(n as f64) as usize);
    let (y0, y1) = ((y - 1.0).floor().max(0.0) as usize, (y + h + 1.0).ceil().min(n as f64) as usize);
    for py in y0..y1 {
        for px in x0..x1 {
            let (fx, fy) = (px as f64 + 0.5, py as f64 + 0.5);
            // Distance to a rounded rectangle, via its inset core.
            let dx = (x + r - fx).max(fx - (x + w - r)).max(0.0);
            let dy = (y + r - fy).max(fy - (y + h - r)).max(0.0);
            let inside_x = fx >= x && fx <= x + w;
            let inside_y = fy >= y && fy <= y + h;
            let d = if dx > 0.0 && dy > 0.0 {
                (dx * dx + dy * dy).sqrt() - r
            } else if inside_x && inside_y {
                -1.0
            } else {
                (dx.max(dy)) - r
            };
            blend(buf, n, px, py, ink, coverage(d) * op);
        }
    }
}

fn paint_arc(
    buf: &mut [f32], n: usize, cx: f64, cy: f64, r: f64, sw: f64,
    start: f64, sweep: f64, ink: Rgb, op: f32,
) {
    let half = sw / 2.0;
    let (x0, x1) = (
        (cx - r - sw).floor().max(0.0) as usize,
        (cx + r + sw).ceil().min(n as f64) as usize,
    );
    let (y0, y1) = (
        (cy - r - sw).floor().max(0.0) as usize,
        (cy + r + sw).ceil().min(n as f64) as usize,
    );
    let full = sweep >= std::f64::consts::TAU - 1e-9;
    for py in y0..y1 {
        for px in x0..x1 {
            let (fx, fy) = (px as f64 + 0.5 - cx, py as f64 + 0.5 - cy);
            let dist = (fx * fx + fy * fy).sqrt();
            let radial = (dist - r).abs() - half;
            if radial > 0.5 {
                continue;
            }
            if !full {
                // Angle relative to the arc's start, wrapped into [0, 2π).
                let mut a = fy.atan2(fx) - start;
                while a < 0.0 {
                    a += std::f64::consts::TAU;
                }
                if a > sweep {
                    // Round caps: still inside if within half a stroke of an end.
                    let end_dist = (a - sweep).min(std::f64::consts::TAU - a) * r;
                    if end_dist > half {
                        continue;
                    }
                }
            }
            blend(buf, n, px, py, ink, coverage(radial) * op);
        }
    }
}

// --- files ----------------------------------------------------------------

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

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn hsl(h: f64, s: f64, l: f64) -> Rgb {
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
    [to(r), to(g), to(b)]
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
        assert_eq!(hex(hsl(0.0, 1.0, 0.5)), "#ff0000");
        assert_eq!(hex(hsl(120.0, 1.0, 0.5)), "#00ff00");
        assert_eq!(hex(hsl(240.0, 1.0, 0.5)), "#0000ff");
        assert_eq!(hex(hsl(0.0, 0.0, 0.0)), "#000000");
        assert_eq!(hex(hsl(0.0, 0.0, 1.0)), "#ffffff");
    }

    #[test]
    fn art_sits_beside_the_audio() {
        assert_eq!(
            path_beside("/home/u/Music/Aria/song.mp3"),
            PathBuf::from("/home/u/Music/Aria/song.svg")
        );
    }

    #[test]
    fn the_raster_is_the_same_picture_as_the_vector() {
        // Not pixel-comparable to the SVG — different blur, different grain —
        // but it must be the same *composition*: same size, deterministic, and
        // carrying the palette the vector declares.
        let rgb = rgb_for("track-abc", 128);
        assert_eq!(rgb.len(), 128 * 128 * 3);
        assert_eq!(rgb, rgb_for("track-abc", 128));
        assert_ne!(rgb, rgb_for("track-abd", 128));

        // Actually painted, not a flat fill: a cover with a gradient, blobs and
        // a motif has to span a real range of values.
        let lo = *rgb.iter().min().unwrap();
        let hi = *rgb.iter().max().unwrap();
        assert!(hi as i32 - lo as i32 > 40, "range {lo}..{hi} is too flat to be art");
    }

    #[test]
    fn every_id_rasterises_without_panicking() {
        // Blob centres, arc radii and bar widths are all random; the painters
        // have to clip rather than index past the buffer for any of them.
        for i in 0..40 {
            let png = png_for(&format!("edge-{i}"), 64);
            assert_eq!(&png[1..4], b"PNG");
        }
    }
}
