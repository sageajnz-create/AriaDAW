# AriaDAW Dev Plan — Hardening & Release (post Phase 10)

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Take Aria from "works on my RX 6650 XT Linux box" to a tested, packaged, downloadable v0.1 that a stranger on any GPU can install and make a song with in under 10 minutes.

**Current context / assumptions**
- Phases 0–10 complete: engine supervisor, simple/studio modes, derive ops (repaint/extend/cover/extract/lego/trim), library + player + playlists + personas + export + video export + follow-along lyrics.
- ~10k LOC (Rust `src-tauri/src/*` ~14 modules, TS `src/*`). 12 Rust test modules exist; no TODOs. Git clean, on `master`, remote `github.com/sageajnz-create/AriaDAW`.
- Primary dev box is Linux/AMD Vulkan. **This machine is Windows** — treat Windows as a first-class new platform, not an afterthought.
- No test framework in `package.json`; no CI config found.

---

## Phase A — Test foundation (highest leverage, lowest glamour)

**Objective:** Lock in behavior so refactors and ports stop being scary.

1. **Frontend test harness** — add Vitest + Testing Library.
   - Files: `package.json`, new `src/__tests__/`
   - Targets: `src/timing.ts` (lyrics time estimation — pure functions, easy wins), `src/player.ts` queue logic (shuffle/repeat/next-prev), `src/types.ts` consumers.
2. **Rust unit coverage for the format-sensitive code**
   - `src-tauri/src/trim.rs` already has verified MP3/WAV frame logic — port those manual verifications into `#[cfg(test)]` fixtures (real LAME-encoded sample frames committed under `src-tauri/tests/fixtures/`).
   - `src-tauri/src/art.rs`: golden-file tests — render N covers via both SVG and raster paths, assert byte-stability across runs (the Phase 8 regression you already did manually).
   - `src-tauri/src/library.rs`: lineage tree integrity — deleting a track detaches children correctly, playlist deletes never touch audio.
3. **Engine contract tests** — mock server (wiremock) asserting request/response shapes for `POST /lm`, `/synth`, `/understand`, `/vae`, `/health`, plus DeviceLost retry-on-smaller-chunk behavior in `src-tauri/src/client.rs`.
4. **CI** — GitHub Actions: `cargo fmt --check && cargo clippy -D warnings && cargo test` + `tsc && vite build` on ubuntu-latest; matrix job adding windows-latest.

## Phase B — Windows port (this machine)

**Objective:** `cargo tauri dev` and `cargo tauri build` working natively on Windows.

5. Audit `src-tauri` for Unix-isms: path separators in `library.rs`/`export.rs`, signal handling in `engine.rs` supervisor (`terminate` → use Tauri/KillOnDrop on Windows), ffmpeg detection in `video.rs` (`where` vs `which`), WebpackGTK-specific playback notes don't apply but WebView2 audio needs verifying.
6. Verify model download paths land in correct `%LOCALAPPDATA%` dirs (`models.rs`); confirm long-path handling for GGUF files.
7. Build NSIS/MSI installer via `tauri.conf.json` bundle targets; smoke-test full loop: setup wizard → VRAM detect → download tier → generate → play → export.
8. Document Windows requirements (WebView2, Vulkan runtime) in README alongside the existing Linux GStreamer note.

## Phase C — First-run & failure UX

9. **Auto-tune VAE chunk on Windows too** — the RADV watchdog finding must have an equivalent guard for other drivers; ensure probe-and-halve logic in `engine.rs` runs per-machine on first generation regardless of OS.
10. **Error taxonomy** — every user-visible failure (device lost, disk full, download interrupted mid-checksum, missing ffmpeg) maps to one plain-language message with a next action. Centralize in a new `src/errors.ts` + Rust error enum rather than ad-hoc strings.
11. **Resume/integrity pass** on `models.rs` downloader: interrupted download resumes at byte offset; corrupted file fails checksum and re-downloads once before reporting honestly.

## Phase D — Release v0.1

12. Version bump to 0.1.0 release, changelog from git log (commit messages are already user-facing quality — keep that convention).
13. Package matrix: Linux AppImage/.deb (existing `scripts/package.sh`), Windows installer. Attach to GitHub Releases.
14. README quickstart per OS; screenshots/GIF of simple mode generating a song.
15. Accessibility spot-audit vs the PLAN.md commitments (focus rings, aria-live on generation, reduced-motion) — it's a stated principle, verify before strangers arrive.

## Risks / tradeoffs
- **Windows testing needs real GPU variance** — I can verify build/install here; Vulkan behavior on NVIDIA/Intel cards needs tester feedback. Mitigation: CI + GitHub Issues template asking for `vulkaninfo` output.
- **acestep.cpp churn** — vendored pinned commit stays frozen until v0.1 ships.
- Scope discipline: no new features (no sharing/publishing — already deliberately rejected). Everything above protects what exists.

## Open questions
- Do you want macOS support in v0.1 or defer? (Metal backend exists upstream; signing/notarization is the real cost.)
- Auto-update story: none (fully offline ethos) or check-for-update prompt only?
