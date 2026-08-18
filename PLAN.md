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

Models load in stages — the LM is unloaded before the DiT — so peak VRAM is set by the
largest single model, not the sum on disk.

| Tier | LM | DiT | Peak VRAM | Disk |
|---|---|---|---|---|
| **Light** (≥4 GB) | 0.6B Q8_0 | turbo Q6_K | ~2 GB | ~3.8 GB |
| **Standard** (≥6 GB) | 1.7B Q8_0 | turbo Q8_0 | ~3 GB | ~5.6 GB |
| **Best** (≥8 GB) ← this box | 4B Q8_0 | sft Q8_0 | **7.0 GB measured** | ~12 GB |

**The 4B does not fit a 6 GB card.** Measured at 7048 MB of 8176 on the RX 6650 XT, so
8 GB is the real floor for the best tier. Smaller cards get the 1.7B, which is
materially worse — see below.

### Quality is a model-pair choice, and it is user-visible

Measured on a 60-second song:

| | Fast | Best |
|---|---|---|
| Models | smallest LM + turbo, 8 steps | largest LM + SFT, 50 steps |
| Time | ~23 s | ~55 s |

Best is the default. Unlimited local generation means a slower setting only spends
patience, so there is no reason to default to the cheap path the way a metered service
would.

Nobody should have to read any of this. The installer detects VRAM, picks a tier, and
says how long a song will take in plain language.

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

### Phase 0 — Engine spike ✅ **PASSED** *(2026-08-15)*

Built acestep.cpp with Vulkan on the RX 6650 XT and generated a complete 60-second
indie-folk track with sung vocals. Verified real audio: 5,746,176 samples,
mean −15.2 dB, peak 0.0 dB, 48 kHz stereo.

**Measured on the 6650 XT (60s song, turbo, 8 steps):**

| Stage | Time |
|---|---|
| LM — lyrics, metadata, 299 audio codes | 8.3 s (86 tok/s) |
| DiT — 8 steps, 1937-node graph | 2.2 s |
| VAE decode — 8 tiles | 4.2 s |
| MP3 encode | 1.0 s (58× realtime) |
| **Total** | **≈ 17 s** |

**This is interactive speed.** ~17 s for a full minute of music means generation can
feel immediate — the UI does *not* need to be queue-first. Jobs still run in the
background and stay cancellable, but the design target is "watch it appear", not
"come back later". Peak VRAM stayed under 2 GB, so the 8 GB budget is generous and
the XL tier is plausible later.

#### Critical finding: the default VAE chunk hangs RADV

At the stock `--vae-chunk 1024`, VAE decode built a 479-node graph over 960 latent
frames and **lost the GPU device**:

```
radv/amdgpu: The CS has been cancelled because the context is lost.
terminate called after throwing an instance of 'vk::DeviceLostError'
```

The kernel runs long enough to trip RADV's watchdog. `--vae-chunk 256 --vae-overlap 32`
splits it into 8 short tiles and succeeds, costing only ~4.2 s. The GPU hard-recovered
both times with no lasting harm.

**Product requirements this creates:**

1. Ship a **conservative VAE chunk by default on Vulkan/RADV** — never the upstream 1024.
2. **Auto-tune on first run**: probe a decode, and on `DeviceLost` halve the chunk and
   retry. Store the working value per machine.
3. **Never surface a device-lost crash to the user.** Catch it, retry smaller, and if it
   still fails, fall back to CPU decode with an honest message.

A user whose first song crashes their GPU driver never opens the app again. This single
finding justifies the whole phase-0 gate.

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

### Phase 5 — The shell around the model

Phases 0–3 got the *capabilities* past Suno. What was still missing was the part
that makes a generator feel like a music app: somewhere for the songs to live,
a way to play them, and a way to find one again a month later.

Done:

- **Cover art for every song.** Drawn deterministically from the track id in
  `src-tauri/src/art.rs` — no image model, no second download, no GPU time, and
  the same picture on every machine forever. Written beside the audio as an
  ordinary SVG the user keeps, and served from the local audio server at
  `/art/<id>`.
- **One player with a queue.** Play-through, next/previous, shuffle, repeat
  (off/all/one), seek and volume. Replaces the per-row `<audio>` elements, which
  meant there was no such thing as "playing your songs", only playing one song.
- **Search, filter and order.** Searches titles, prompts, styles *and lyrics* —
  you remember a line from a song far more often than what you called the file.
- **Playlists.** Named, ordered, reorderable; a song can sit in several at once.
  Deleting a playlist never deletes music, and deleting a song takes it out of
  every playlist it was in.

### Phase 6 — Saved voices, and getting your music out

A persona is a singer you named and kept. The engine already took timbre from a
reference without borrowing its notes or words; what was missing was the ability
to *keep* one, rather than scrolling a dropdown of every song you own and hoping
you remember which one sounded right.

Aria's version differs from Suno's in the way that matters: a persona holds its
**own copy** of the reference, in the app's data folder. A voice you named and
reused should not quietly stop working the day you tidy up the song it came
from, so `source_track_id` is provenance only and deliberately not a foreign
key. The cached latent is preferred over the audio where there is one — it is
what the engine actually consumes, it is far smaller, and reusing it skips a VAE
encode on every song the persona sings.

A persona also carries the tempo and key it was captured with, applied only
where the user left those controls on automatic. A saved singer supplies
defaults; it does not overrule a stated intent.

**Export** closes the other half of the ownership promise. The library folder
already holds plain files, but every one of them is named by uuid — a fine
primary key and a terrible thing to hand someone who wants to put a playlist on
a phone. Export copies whatever is on screen (so a search or a filter narrows it
too), numbered in running order under readable names, with each cover beside its
song and an `.m3u` using relative paths so the folder still plays after it is
moved. A track whose file has been moved outside the app is reported and
skipped rather than failing the whole run.

### Phase 7 — Remaining parity gaps

| Gap | Notes |
|---|---|
| Follow-along lyrics | Suno scrolls the words with playback. The engine gives us no timing data, so this needs either forced alignment or an honest per-section estimate — not a fake one. |
| Trim / crop | Cut a song down to a section and keep it as its own track. |
| Video export | Suno's shareable MP4 is cover art plus audio. Needs ffmpeg, which is a real dependency decision, not a small one. |

Deliberately **not** pursued: publishing, sharing feeds, and public profiles.
Those need a server, and a server is the thing Aria exists not to have.

---

## Risks

| Risk | Mitigation |
|---|---|
| GGML Vulkan unstable on RADV/Navi23 | Phase 0 gate; CPU fallback path kept working |
| 8 GB VRAM tight at Standard tier | Staged load (LM then DiT); Light tier default |
| Generation too slow to feel good | Turbo DiT (8 steps); queue + background jobs so UI never blocks |
| Upstream acestep.cpp churn | Vendored at a pinned commit; we control when to bump |
| Output quality below Suno | Tier switching + seed control; XL tier for stronger GPUs |
