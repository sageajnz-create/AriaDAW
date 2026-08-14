# Aria — Free, unlimited, local AI music creation

**Your music. Your machine. No limits.**

Aria generates complete songs — with real sung vocals — entirely on your own computer.
No account. No credits. No queue. No subscription. No one owns your output but you.

---

## Why this can exist

Suno's free tier gives you ~10 songs a day, forbids commercial use, and **Suno owns
what you make on it**. That last part is the real problem — not the credit count.

The reason Suno can charge is that they own the model. That moat is gone:

- **ACE-Step 1.5** (ACE Studio + StepFun) is an MIT-licensed music foundation model
  that generates full songs with vocals in 50+ languages, 10 seconds to 10 minutes.
- **acestep.cpp** is an MIT C++17/GGML port with quantized GGUF weights that runs on
  **CPU, CUDA, ROCm, Metal, and Vulkan**.

So the model is free, the runtime is free, and it runs on hardware people already own.
The only thing missing is a product that isn't a research demo. That's Aria.

### Legal footing

Every adverse ruling in the AI-music lawsuits has turned on **training** on scraped
audio. Warner settled with Suno, UMG with Udio; Sony litigation continues. Aria trains
nothing and scrapes nothing — it runs an MIT-licensed model locally. Our exposure is
categorically different from Suno's, and generation never leaves the user's machine.

---

## Target hardware (verified on this box)

| | |
|---|---|
| GPU | AMD Radeon RX 6650 XT (Navi 23, `gfx1032`), 8176 MB VRAM (~6.9 GB usable) |
| Driver | `amdgpu` + Mesa RADV, Vulkan 1.4 |
| CPU / RAM | 12 cores, 15 GB |

**We use Vulkan, not ROCm — deliberately.** Navi 23 is `gfx1032`, which ROCm does not
officially support; making it work needs `HSA_OVERRIDE_GFX_VERSION=10.3.0` and constant
fighting. GGML's Vulkan backend runs on the stock RADV driver already installed. This
also means Aria works on *any* reasonably modern GPU — AMD, Intel, or NVIDIA — with no
vendor runtime to install. That is essential for a tool meant to be free to everyone.

### Model tiers (auto-selected from detected VRAM)

| Tier | DiT | LM | Encoder | VAE | Total |
|---|---|---|---|---|---|
| **Light** (≥4 GB) | turbo Q6_K 1.97 GB | 0.6B Q8_0 710 MB | Q8_0 784 MB | BF16 322 MB | **~3.8 GB** |
| **Standard** (≥8 GB) ← this box | turbo Q8_0 2.55 GB | 1.7B Q8_0 1.98 GB | Q8_0 784 MB | BF16 322 MB | **~5.6 GB** |
| **Quality** (≥16 GB) | xl-sft Q8_0 5.31 GB | 4B Q8_0 4.46 GB | Q8_0 784 MB | BF16 322 MB | **~10.9 GB** |
| **CPU fallback** | turbo Q4_K_M 1.45 GB | 0.6B Q8_0 710 MB | Q8_0 784 MB | BF16 322 MB | **~3.3 GB** |

Nobody should have to read this table. The installer detects VRAM, picks a tier, and
says "this will take about X minutes per song" in plain language.

---

## Architecture

Aria **wraps** acestep.cpp rather than reimplementing it. Upstream keeps improving the
model; we own the product. The engine runs as a local HTTP server on `127.0.0.1`,
supervised by the Tauri Rust process.

```
AriaDAW/
├─ engine/acestep.cpp/     vendored upstream (MIT), built with Vulkan
├─ src-tauri/              Rust
│  ├─ engine/              process supervisor: spawn, health, restart, shutdown
│  ├─ models/              downloader (resumable, checksummed), VRAM tier picker
│  ├─ jobs/                generation queue + progress events → UI
│  ├─ library/             SQLite: tracks, lineage, tags
│  └─ audio/               export wav/mp3/flac, stems via Demucs sidecar
├─ src/                    React + TypeScript
│  ├─ modes/simple/        one box, one button
│  ├─ modes/studio/        lyrics, style, BPM/key/duration/seed
│  ├─ library/             browse, play, derive, export
│  └─ a11y/                live regions, focus management, preferences
└─ docs/
```

**Engine API** (`acestep.cpp` server, port 8085): `POST /lm` (caption → lyrics + audio
codes), `POST /synth` (codes → MP3/WAV), `POST /understand` (audio → metadata + lyrics),
`POST /vae`, `GET /health`, `GET /props`.

Native task types: `text2music`, `cover`, `repaint`, `lego`, `extract`, `complete`.

**Stem separation is native after all — Demucs is not needed.** `extract` isolates a
stem from a mix and supports **twelve** track types (`vocals`, `backing_vocals`,
`drums`, `bass`, `guitar`, `keyboard`, `percussion`, `strings`, `synth`, `fx`, `brass`,
`woodwinds`) against Demucs' four. That removes the Python sidecar entirely: one
engine, one language, no extra runtime for users to install.

Two corrections from reading the engine docs closely:

- **`complete` is not temporal extend.** It generates a full mix from an isolated stem
  (a cappella → full song) and explicitly "does NOT splice or extend temporally."
  Real extend is **`repaint` with `repainting_start < 0` or `end` beyond source
  duration** — i.e. outpainting.
- **`lego`, `extract`, and `complete` require the Base/SFT DiT, not turbo.** Turbo only
  covers `text2music`, `cover`, and `repaint`.

| Feature | Task | DiT required | Steps |
|---|---|---|---|
| Generate a song | `text2music` | turbo ✅ | 8 |
| Cover / restyle | `cover` | turbo ✅ | 8 |
| Regenerate a section | `repaint` | turbo ✅ | 8 |
| Extend a track | `repaint` (outpaint) | turbo ✅ | 8 |
| Split into stems | `extract` | **SFT** | 50 |
| Add an instrument layer | `lego` | **SFT** | 50 |
| Stem → full mix | `complete` | **SFT** | 50 |

So Aria ships both DiTs (~2.55 GB each). Turbo drives the fast interactive path; SFT
loads only for stem-level work, where 50 steps cost ~6× turbo's time. The UI must set
that expectation honestly rather than appearing to hang.

`lego` deserves attention — adding a named instrument layer to existing audio is a
genuinely DAW-shaped capability that Suno has no equivalent for.

### Library data model

Tracks carry `parent_id` + `operation`, giving every song a **lineage tree** — the cover
of a repaint of an extend stays connected to its source. This is a genuine improvement
over Suno's flat history, and it costs us nothing to build correctly from the start.

Audio files live in a plain, user-visible folder. The database is an index, never a
cage. Delete Aria and your music is still there, as ordinary files.

---

## Design principles

1. **Free means free.** No accounts, no telemetry, no network calls after model
   download. There is no paywall to add later because there is no server to meter.
2. **You own it.** Files on disk in open formats. MIT model, so commercial use is fine.
3. **Accessible by construction.** Built for people who can't make music the usual way —
   including people using a screen reader or a keyboard only.
4. **Two doors.** *Simple mode* is one text box and one button. *Studio mode* exposes
   lyrics, key, BPM, seed. Nobody is forced through complexity to get a song.
5. **Honest about time.** Local generation takes as long as it takes. Show real progress,
   never block the UI, announce completion.

### Accessibility commitments (from day one, not retrofitted)

- Every control keyboard-reachable with a visible focus ring
- `aria-live` announcements for generation start, progress, and completion
- Labels and roles on all controls; no icon-only buttons without accessible names
- `prefers-reduced-motion` respected for waveforms and transitions
- WCAG AA contrast minimum, plus a larger-text option
- No time-limited interactions anywhere
- Plain language throughout — no unexplained producer jargon

---

## Build phases

### Phase 0 — Engine spike *(de-risk before any UI)*
Build acestep.cpp with Vulkan, fetch the Light tier, generate one song from the CLI on
the 6650 XT, measure seconds-per-song.
**Gate:** if Vulkan/RADV misbehaves, fall back to CPU and re-scope timings honestly.
Nothing else is worth building until this works.

### Phase 1 — Skeleton + core loop
Tauri shell, engine supervisor, first-run model download with VRAM detection,
Simple mode: prompt → song → plays → saved to library.

### Phase 2 — Studio controls
Lyrics editor with structure tags (`[verse]`, `[chorus]`, `[bridge]`), style prompt,
BPM / key / duration / language / seed, model tier switching.

### Phase 3 — Derived operations
Repaint a section, extend a track, generate a cover (all native), stems via Demucs.
Lineage tree in the library UI.

### Phase 4 — Accessibility audit + packaging
Full keyboard and screen-reader pass, contrast audit, AppImage / .deb / Flatpak,
polished first-run experience.

---

## Risks

| Risk | Mitigation |
|---|---|
| GGML Vulkan unstable on RADV/Navi23 | Phase 0 gate; CPU fallback path kept working |
| 8 GB VRAM tight at Standard tier | Staged load (LM then DiT); Light tier default |
| Generation too slow to feel good | Turbo DiT (8 steps); queue + background jobs so UI never blocks |
| Upstream acestep.cpp churn | Vendored at a pinned commit; we control when to bump |
| Output quality below Suno | Tier switching + seed control; XL tier for stronger GPUs |
