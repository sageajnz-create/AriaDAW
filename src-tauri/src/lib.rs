//! Aria — free, unlimited, local AI music creation.

mod audio_server;
mod client;
mod derive;
mod engine;
mod library;
mod lyrics;
mod models;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use client::{AceClient, JobStatus, SynthSources};
use engine::{AvailableModels, Engine, EnginePaths, EngineSettings, EngineState};
use library::{Library, Track};

/// How many times we'll shrink the VAE chunk and retry after a GPU device loss
/// before giving up on the GPU and decoding on the CPU.
const MAX_DEVICE_LOST_RETRIES: usize = 3;

pub struct AppState {
    engine: Mutex<Engine>,
    library: Arc<Mutex<Library>>,
    settings_path: PathBuf,
    /// Base URL of the local audio server, e.g. http://127.0.0.1:41234/<token>
    audio_base: String,
}

#[derive(Debug, Serialize)]
pub struct EngineStatus {
    state: EngineState,
    models: AvailableModels,
    models_complete: bool,
    supports_stems: bool,
    vae_chunk: u32,
    cpu_fallback: bool,
}

#[derive(Debug, Deserialize)]
pub struct GenerateOptions {
    pub prompt: String,
    /// Empty means "let the model write the lyrics".
    #[serde(default)]
    pub lyrics: String,
    #[serde(default = "default_duration")]
    pub duration: f64,
    #[serde(default)]
    pub instrumental: bool,
    /// Let the model expand the description into a richer caption of its own.
    ///
    /// **On by default.** Turning it off fixes language selection but costs far
    /// more than it buys, measured on real output:
    ///
    /// | | expansion on | expansion off |
    /// |---|---|---|
    /// | caption the DiT sees | rich, 160+ chars of musical detail | the user's bare prompt |
    /// | lyrics | real words, 525-655 chars | skeletal stubs, e.g. `[Verse 1] [Male vocal ad-lib: Woo!]` |
    /// | vocals | yes | often none at all |
    ///
    /// A song with no singing and thin production is a worse failure than a song
    /// in an unexpected language, so expansion stays on and language is nudged
    /// through the caption instead.
    #[serde(default = "default_true")]
    pub embellish: bool,
    #[serde(default)]
    pub bpm: Option<i64>,
    #[serde(default)]
    pub keyscale: Option<String>,
    #[serde(default)]
    pub timesignature: Option<String>,
    #[serde(default)]
    pub vocal_language: Option<String>,
    #[serde(default)]
    pub seed: Option<i64>,
    /// "fast" (turbo, 8 steps) or "best" (SFT, 50 steps).
    #[serde(default)]
    pub quality: Option<String>,
    /// Sampling temperature for lyric writing. Higher wanders more and repeats
    /// more; lower is plainer but coherent.
    #[serde(default)]
    pub lyric_variety: Option<f64>,
    /// How many takes to make from one prompt. Same words, different
    /// performances — Suno gives two and calls it a feature; there is no reason
    /// to ration it here beyond the time it costs.
    #[serde(default)]
    pub variations: Option<u8>,
    /// "mp3" (320 kbps) or "wav24" / "wav16" / "wav32" for lossless.
    #[serde(default)]
    pub format: Option<String>,
    /// Id of a track whose singer this song should sound like.
    ///
    /// The engine takes timbre from the reference without borrowing its notes
    /// or words, so one voice can carry across a whole set of songs. This is
    /// the honest equivalent of Suno's Personas, except the reference is a
    /// track you already own rather than an account-bound artifact.
    #[serde(default)]
    pub voice_from: Option<String>,
}

fn default_duration() -> f64 {
    60.0
}

fn default_true() -> bool {
    true
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The language songs should be sung in when the user hasn't picked one.
///
/// Left unset, the engine's expansion step *invents* a language, and on an
/// unusual prompt it wanders — an English prompt about reggae came back sung in
/// Hindi. Singing in 50+ languages is a feature; picking one at random is not.
/// Defaulting to the user's own locale is the least surprising behaviour, and
/// the UI still lets them choose any language deliberately.
fn default_vocal_language() -> String {
    let raw = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();
    parse_locale(&raw)
}

/// "en_NZ.UTF-8" -> "en". Falls back to English for `C`, `POSIX`, or anything
/// that isn't a two-letter code.
fn parse_locale(raw: &str) -> String {
    let code = raw
        .split(['_', '.', '@'])
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()) {
        code
    } else {
        "en".to_string()
    }
}

// ---------------------------------------------------------------- settings io

fn load_settings(path: &PathBuf) -> EngineSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(path: &PathBuf, s: &EngineSettings) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(txt) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(path, txt);
    }
}

// ------------------------------------------------------------------- commands

// Tauri runs non-async commands on the main thread, which is also the GTK event
// loop. Anything slow there stops the window repainting — it goes grey, exactly
// as reported. `start_engine` can block for a minute loading multi-gigabyte
// models, and `engine_status` waits on the same mutex that a running generation
// holds. Both move to a blocking pool so the UI keeps painting.

#[tauri::command]
async fn engine_status(state: State<'_, Arc<AppState>>) -> Result<EngineStatus, String> {
    let st = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let eng = st.engine.lock().unwrap();
        let models = eng.paths.available_models().unwrap_or_default();
        EngineStatus {
            state: eng.state(),
            models_complete: models.is_complete(),
            supports_stems: models.supports_stems(),
            models,
            vae_chunk: eng.settings.vae_chunk,
            cpu_fallback: eng.settings.cpu_fallback,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_engine(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let st = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        st.engine.lock().unwrap().start().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Off the main thread: this queries SQLite and stats every track's file to
/// work out whether it still exists, and it runs on every library refresh.
#[tauri::command]
async fn list_tracks(
    state: State<'_, Arc<AppState>>,
    limit: Option<i64>,
) -> Result<Vec<Track>, String> {
    let st = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        st.library.lock().unwrap().list(limit.unwrap_or(500)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn set_favorite(state: State<'_, Arc<AppState>>, id: String, favorite: bool) -> Result<(), String> {
    state.library.lock().unwrap().set_favorite(&id, favorite).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_track(state: State<'_, Arc<AppState>>, id: String, title: String) -> Result<(), String> {
    state.library.lock().unwrap().rename(&id, &title).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_track(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.library.lock().unwrap().delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn library_folder(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(state.library.lock().unwrap().audio_dir.display().to_string())
}

/// Bring an existing recording into the library.
///
/// This is the thing Suno's free tier will not do at all. Once a song is in,
/// every derived operation works on it — pull the drums out of a demo, restyle
/// a voice memo, extend a loop you made years ago. The engine analyses it for
/// tempo, key, a description of how it sounds, and any lyrics it can hear.
#[tauri::command]
fn import_audio(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<String, String> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let st = Arc::clone(&state);
    let jid = job_id.clone();

    std::thread::spawn(move || {
        if let Err(e) = run_import(&app, &st, &jid, &path) {
            let _ = app.emit("gen:error", json!({ "job_id": jid, "message": e.to_string() }));
        }
    });
    Ok(job_id)
}

fn run_import(
    app: &AppHandle,
    st: &Arc<AppState>,
    job_id: &str,
    source_path: &str,
) -> anyhow::Result<()> {
    let src = std::path::Path::new(source_path);
    let name = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Imported song".into());

    let audio = std::fs::read(src)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", src.display()))?;

    {
        let mut eng = st.engine.lock().unwrap();
        if eng.state() != EngineState::Ready || eng.has_died() {
            emit_stage(app, job_id, "starting", "Waking the engine");
            eng.restart()?;
        }
    }
    let ace = AceClient::new(st.engine.lock().unwrap().base_url())?;

    emit_stage(app, job_id, "composing", "Listening to your song");
    let filename = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "audio".into());
    let job = ace.submit_understand(audio.clone(), &filename)?;
    wait_for(&ace, &job, app, job_id, "composing", "Listening to your song")?;
    let (analysis, latent) = ace.understand_result(&job)?;

    emit_stage(app, job_id, "saving", "Adding it to your library");
    let id = uuid::Uuid::new_v4().to_string();
    let lib = st.library.lock().unwrap();

    // Copy rather than reference: the library owns its files, and the original
    // stays exactly where the user put it.
    let ext = src.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "mp3".into());
    let audio_path = lib.audio_dir.join(format!("{id}.{ext}"));
    std::fs::write(&audio_path, &audio)?;
    let latent_path = latent.as_ref().and_then(|l| {
        let p = lib.audio_dir.join(format!("{id}.latent"));
        std::fs::write(&p, l).ok().map(|_| p.display().to_string())
    });

    let track = Track {
        id,
        title: name,
        prompt: String::new(),
        caption: analysis.get("caption").and_then(Value::as_str).unwrap_or_default().to_string(),
        lyrics: analysis.get("lyrics").and_then(Value::as_str).unwrap_or_default().to_string(),
        bpm: analysis.get("bpm").and_then(Value::as_i64),
        keyscale: analysis.get("keyscale").and_then(Value::as_str).map(String::from),
        timesignature: analysis.get("timesignature").and_then(Value::as_str).map(String::from),
        vocal_language: analysis.get("vocal_language").and_then(Value::as_str).map(String::from),
        duration: analysis.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
        seed: None,
        model: String::new(),
        audio_path: audio_path.display().to_string(),
        latent_path,
        created_at: now_secs(),
        parent_id: None,
        operation: Some("imported".into()),
        favorite: false,
        lyricist: None,
        missing: false,
    };
    lib.insert(&track)?;
    drop(lib);

    let _ = app.emit("gen:done", json!({ "job_id": job_id, "track": track }));
    Ok(())
}

/// What the first-run setup needs to show: which tier suits this machine, how
/// much still has to be downloaded, and whether anything is missing.
#[derive(Debug, Serialize)]
pub struct SetupInfo {
    tier: models::Tier,
    tier_label: &'static str,
    tier_description: &'static str,
    vram_mb: Option<u64>,
    total_bytes: u64,
    missing_bytes: u64,
    missing_count: usize,
    ready: bool,
}

#[tauri::command]
async fn setup_info(
    state: State<'_, Arc<AppState>>,
    tier: Option<models::Tier>,
) -> Result<SetupInfo, String> {
    let st = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let dir = st.engine.lock().unwrap().paths.models_dir.clone();
        let tier = tier.unwrap_or_else(models::suggested_tier);
        let missing = models::missing_files(&dir, tier);
        SetupInfo {
            tier,
            tier_label: tier.label(),
            tier_description: tier.description(),
            vram_mb: models::detect_vram_mb(),
            total_bytes: tier.total_bytes(),
            missing_bytes: missing.iter().map(|f| f.bytes).sum(),
            missing_count: missing.len(),
            ready: missing.is_empty(),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Fetch whatever a tier is missing. Progress arrives as `setup:progress`, and
/// it ends with `setup:done` or `setup:error`.
#[tauri::command]
fn download_models(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    tier: models::Tier,
) -> Result<(), String> {
    let st = Arc::clone(&state);
    let dir = st.engine.lock().unwrap().paths.models_dir.clone();

    std::thread::spawn(move || {
        let app2 = app.clone();
        let result = models::download_tier(
            &dir,
            tier,
            move |p| {
                let _ = app2.emit("setup:progress", &p);
            },
            || false,
        );
        match result {
            Ok(()) => {
                // Restart so the engine picks up what just arrived.
                let mut eng = st.engine.lock().unwrap();
                eng.stop();
                drop(eng);
                let _ = app.emit("setup:done", json!({}));
            }
            Err(e) => {
                let _ = app.emit("setup:error", json!({ "message": e.to_string() }));
            }
        }
    });
    Ok(())
}

/// Where the webview should fetch audio from.
#[tauri::command]
fn audio_base_url(state: State<'_, Arc<AppState>>) -> String {
    state.audio_base.clone()
}

#[tauri::command]
fn stem_choices() -> Vec<derive::StemChoice> {
    derive::stem_choices()
}

/// Record a UI error to a file next to the library.
///
/// A desktop window has no console anyone can open, so a JavaScript error is
/// invisible — the screen just goes blank. Writing them down turns "it went
/// grey" into something with a stack trace.
#[tauri::command]
fn log_ui_error(state: State<'_, Arc<AppState>>, message: String, stack: Option<String>) {
    let path = state
        .settings_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("ui-errors.log");
    let entry = format!(
        "\n=== {} ===\n{}\n{}\n",
        now_secs(),
        message,
        stack.unwrap_or_default()
    );
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(entry.as_bytes());
    }
    eprintln!("[ui error] {message}");
}

/// Make a new track from an existing one: isolate a stem, restyle it, redo a
/// section, or make it longer. Same event contract as `generate`.
#[tauri::command]
fn derive_track(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
    operation: derive::Operation,
) -> Result<String, String> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let st = Arc::clone(&state);
    let jid = job_id.clone();

    std::thread::spawn(move || {
        if let Err(e) = run_derive(&app, &st, &jid, &id, operation) {
            let _ = app.emit("gen:error", json!({ "job_id": jid, "message": e.to_string() }));
        }
    });
    Ok(job_id)
}

fn run_derive(
    app: &AppHandle,
    st: &Arc<AppState>,
    job_id: &str,
    source_id: &str,
    op: derive::Operation,
) -> anyhow::Result<()> {
    {
        let mut eng = st.engine.lock().unwrap();
        if eng.state() != EngineState::Ready || eng.has_died() {
            emit_stage(app, job_id, "starting", "Waking the engine");
            eng.restart()?;
        }
    }
    let ace = AceClient::new(st.engine.lock().unwrap().base_url())?;

    let source = st
        .library
        .lock()
        .unwrap()
        .get(source_id)?
        .ok_or_else(|| anyhow::anyhow!("that track is no longer in your library"))?;

    let models = st.engine.lock().unwrap().paths.available_models()?;
    let dit = if op.needs_sft() {
        models.dit_sft.first().cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "This needs the detailed sound model, which isn't installed yet."
            )
        })?
    } else {
        models
            .dit_turbo
            .first()
            .cloned()
            .or_else(|| models.dit_sft.first().cloned())
            .ok_or_else(|| anyhow::anyhow!("no diffusion model installed"))?
    };
    // SFT-only tasks are 50-step; turbo tasks stay at 8.
    let steps = if op.needs_sft() { 50 } else { 8 };

    emit_stage(app, job_id, "rendering", &describe(&op));

    let mut req = json!({
        "synth_model": dit,
        "task_type": op.task_type(),
        "caption": source.caption,
        "lyrics": source.lyrics,
        "inference_steps": steps,
        "guidance_scale": 1.0,
        "shift": if op.needs_sft() { 1.0 } else { 3.0 },
        "mp3_bitrate": 320,
        "peak_clip": 0,
        "audio_codes": "",
    });

    match &op {
        derive::Operation::Stem { track } => {
            req["track"] = json!(track);
            // Isolation shouldn't be steered by the original text.
            req["lyrics"] = json!("");
        }
        derive::Operation::AddLayer { track, caption } => {
            req["track"] = json!(track);
            if let Some(c) = caption {
                req["caption"] = json!(c);
            }
            req["lyrics"] = json!("[Instrumental]");
        }
        derive::Operation::Cover { caption, strength } => {
            req["caption"] = json!(caption);
            req["audio_cover_strength"] = json!(strength);
        }
        derive::Operation::Repaint { start, end, caption } => {
            req["repainting_start"] = json!(start);
            req["repainting_end"] = json!(end);
            if let Some(c) = caption {
                req["caption"] = json!(c);
            }
        }
        derive::Operation::Extend { seconds } => {
            // Outpainting: paint from the old end out to the new one.
            req["repainting_start"] = json!(source.duration);
            req["repainting_end"] = json!(source.duration + seconds);
        }
    }

    // Replay the cached latent when we have it and skip a VAE encode.
    let latent = source
        .latent_path
        .as_ref()
        .and_then(|p| std::fs::read(p).ok());
    let audio = if latent.is_none() { std::fs::read(&source.audio_path).ok() } else { None };

    let job = ace.submit_synth_with_source(
        &req,
        SynthSources { audio, latent, ..Default::default() },
    )?;
    wait_for(&ace, &job, app, job_id, "rendering", &describe(&op))?;
    let out = ace.synth_result(&job)?;

    emit_stage(app, job_id, "saving", "Saving to your library");
    let new_id = uuid::Uuid::new_v4().to_string();
    let lib = st.library.lock().unwrap();
    let audio_path = lib.audio_dir.join(format!("{new_id}.mp3"));
    std::fs::write(&audio_path, &out.audio)?;
    let latent_path = out.latent.as_ref().and_then(|l| {
        let p = lib.audio_dir.join(format!("{new_id}.latent"));
        std::fs::write(&p, l).ok().map(|_| p.display().to_string())
    });

    let track = Track {
        id: new_id,
        title: op.title_for(&source.title),
        prompt: source.prompt.clone(),
        caption: source.caption.clone(),
        // A stem has no words of its own.
        lyrics: if matches!(op, derive::Operation::Stem { .. }) {
            String::new()
        } else {
            source.lyrics.clone()
        },
        bpm: source.bpm,
        keyscale: source.keyscale.clone(),
        timesignature: source.timesignature.clone(),
        vocal_language: source.vocal_language.clone(),
        duration: match &op {
            derive::Operation::Extend { seconds } => source.duration + seconds,
            _ => source.duration,
        },
        seed: None,
        model: dit,
        audio_path: audio_path.display().to_string(),
        latent_path,
        created_at: now_secs(),
        parent_id: Some(source.id.clone()),
        operation: Some(op.label()),
        favorite: false,
        lyricist: None,
        missing: false,
    };
    lib.insert(&track)?;
    drop(lib);

    let _ = app.emit("gen:done", json!({ "job_id": job_id, "track": track }));
    Ok(())
}

fn describe(op: &derive::Operation) -> String {
    match op {
        derive::Operation::Stem { .. } => "Separating the parts".into(),
        derive::Operation::Cover { .. } => "Reimagining the song".into(),
        derive::Operation::Repaint { .. } => "Redoing that section".into(),
        derive::Operation::Extend { .. } => "Making it longer".into(),
        derive::Operation::AddLayer { .. } => "Adding the new part".into(),
    }
}

/// Kick off a generation. Returns immediately with a job id; progress arrives as
/// `gen:stage`, and the run ends with `gen:done` or `gen:error`.
#[tauri::command]
fn generate(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    options: GenerateOptions,
) -> Result<String, String> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let st = Arc::clone(&state);
    let jid = job_id.clone();

    std::thread::spawn(move || {
        if let Err(e) = run_generation(&app, &st, &jid, options) {
            let _ = app.emit(
                "gen:error",
                json!({ "job_id": jid, "message": e.to_string() }),
            );
        }
    });

    Ok(job_id)
}

fn emit_stage(app: &AppHandle, job_id: &str, stage: &str, detail: &str) {
    let _ = app.emit(
        "gen:stage",
        json!({ "job_id": job_id, "stage": stage, "detail": detail }),
    );
}

/// The full two-stage pipeline, with recovery from a lost GPU device.
fn run_generation(
    app: &AppHandle,
    st: &Arc<AppState>,
    job_id: &str,
    opts: GenerateOptions,
) -> anyhow::Result<()> {
    let mut attempt = 0usize;

    loop {
        // --- make sure the engine is up -------------------------------------
        {
            let mut eng = st.engine.lock().unwrap();
            if eng.state() != EngineState::Ready || eng.has_died() {
                emit_stage(app, job_id, "starting", "Waking the engine");
                eng.restart()?;
            }
        }

        let base = st.engine.lock().unwrap().base_url();
        let ace = AceClient::new(&base)?;

        match attempt_generation(app, st, job_id, &opts, &ace) {
            Ok(tracks) => {
                {
                    let mut eng = st.engine.lock().unwrap();
                    eng.mark_chunk_verified();
                    save_settings(&st.settings_path, &eng.settings);
                }
                let _ = app.emit(
                    "gen:done",
                    json!({ "job_id": job_id, "track": tracks.last(), "tracks": tracks }),
                );
                return Ok(());
            }
            Err(e) => {
                // Did the engine die, and did it die from a lost GPU device?
                let (died, device_lost, explanation) = {
                    let mut eng = st.engine.lock().unwrap();
                    (eng.has_died(), eng.crashed_on_device_lost(), eng.explain_failure())
                };

                if !died {
                    return Err(e);
                }

                attempt += 1;
                if attempt > MAX_DEVICE_LOST_RETRIES {
                    return Err(anyhow::anyhow!(
                        "Generation kept failing because {explanation}. \
                         Aria will use slower CPU decoding from now on."
                    ));
                }

                let mut eng = st.engine.lock().unwrap();
                let stepped_down = eng.reduce_chunk();
                save_settings(&st.settings_path, &eng.settings);

                let detail = if stepped_down {
                    format!("Adjusting for your graphics card (attempt {})", attempt + 1)
                } else {
                    "Switching to CPU rendering for reliability".to_string()
                };
                drop(eng);

                if device_lost {
                    emit_stage(app, job_id, "recovering", &detail);
                } else {
                    emit_stage(app, job_id, "recovering", "Restarting the engine");
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

fn attempt_generation(
    app: &AppHandle,
    st: &Arc<AppState>,
    job_id: &str,
    opts: &GenerateOptions,
    ace: &AceClient,
) -> anyhow::Result<Vec<Track>> {
    let models = st.engine.lock().unwrap().paths.available_models()?;

    // Quality picks a *pair* of models, because both halves matter and both
    // cost time. Roughly, on a 60s song:
    //   best — largest LM + SFT diffusion at 50 steps  (~55s)
    //   fast — smallest LM + turbo diffusion at 8 steps (~23s)
    // Best is the default: unlimited local generation means the only thing a
    // slower setting spends is patience, and the whole point is that nobody is
    // metering it.
    let best_quality = opts.quality.as_deref() != Some("fast");

    // --- words -------------------------------------------------------------
    // Prefer a real instruct model for lyrics. ACE-Step's own writer produced
    // 13% distinct lines, drifted between languages, and often sang only
    // vocalisations; llama3.2:3b produced 100% distinct, on-topic lines in the
    // requested language in ~13s. See lyrics.rs.
    let language = opts
        .vocal_language
        .clone()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(default_vocal_language);

    let user_wrote_lyrics = !opts.instrumental && !opts.lyrics.trim().is_empty();
    let mut lyrics = if opts.instrumental {
        "[Instrumental]".to_string()
    } else if user_wrote_lyrics {
        // Someone who writes lyrics without markers still deserves a song with
        // sections. Their words are never changed — only headings are added.
        lyrics::ensure_structure(opts.lyrics.trim())
    } else {
        String::new()
    };
    let mut lyricist: Option<String> = None;
    let mut expanded_style: Option<String> = None;

    let have_instruct_model = lyrics::LyricWriter::detect().is_some();

    if let Some(writer) = lyrics::LyricWriter::detect() {
        // Expand the style request ourselves so the stated genre survives. The
        // engine's own expansion took its cue from the lyrics instead of the
        // request and turned "a modern jazz song" into "theatrical art-pop".
        emit_stage(app, job_id, "writing", "Working out how it should sound");
        if let Ok(style) =
            writer.expand_style(&opts.prompt, &language_name(&language), opts.instrumental)
        {
            expanded_style = Some(style);
        }

        if lyrics.is_empty() {
            emit_stage(app, job_id, "writing", "Writing the words");
            match writer.write(&opts.prompt, &language_name(&language), opts.duration) {
                Ok(text) => {
                    lyricist = Some(writer.model_name().to_string());
                    // Markers are what tell the music where to change, so a
                    // lyric without them gets one rather than being used as-is.
                    lyrics = lyrics::ensure_structure(&text);
                }
                // Not fatal: fall through and let ACE-Step write them instead.
                Err(e) => eprintln!("lyric model failed, falling back: {e}"),
            }
        }
    }

    // No instruct model installed: fall back to ACE-Step's own writer, but
    // don't accept whatever the first attempt produces.
    //
    // That writer is inconsistent rather than uniformly bad. Four runs of one
    // prompt scored 0.05, 0.00, 0.69 and 0.03 — one was genuinely good and the
    // rest were repetition or nothing. `lm_mode: "inspire"` returns lyrics
    // without audio codes in 3-7s, which makes it affordable to ask a few times
    // and keep the best. Most installs will not have Ollama, so this is the
    // path most people actually get.
    if lyrics.is_empty() && !have_instruct_model {
        emit_stage(app, job_id, "writing", "Writing the words");
        let lm_for_words = models
            .best_lm()
            .ok_or_else(|| anyhow::anyhow!("no language model installed"))?;

        let mut best = String::new();
        let mut best_score = 0.0f64;
        for attempt in 0..4 {
            let probe = json!({
                "lm_model": lm_for_words,
                "lm_mode": "inspire",
                "caption": opts.prompt,
                "duration": opts.duration,
                "vocal_language": language,
                "lm_temperature": opts.lyric_variety.unwrap_or(0.45),
                "seed": now_secs().wrapping_mul(31).wrapping_add(attempt) & 0x7fff_ffff,
            });
            let Ok(id) = ace.submit_lm(&probe) else { break };
            if wait_for(ace, &id, app, job_id, "writing", "Writing the words").is_err() {
                break;
            }
            let Ok(results) = ace.lm_results(&id) else { break };
            let text = results
                .first()
                .and_then(|r| r.get("lyrics"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            let score = lyrics::score_lyrics(&text);
            if score > best_score {
                best_score = score;
                best = text;
            }
            if best_score >= lyrics::GOOD_ENOUGH {
                break;
            }
            emit_stage(app, job_id, "writing", "Trying different words");
        }
        if !best.trim().is_empty() {
            lyrics = lyrics::ensure_structure(&best);
        }
    }

    // With lyrics in hand the ACE language model only has to emit audio codes,
    // which the small model does just as well and roughly twice as fast
    // (9s vs 17s measured). Only reach for the large one when it must write.
    let needs_writer = lyrics.is_empty();
    let lm_model = if best_quality && needs_writer {
        models.best_lm()
    } else {
        models.fastest_lm().or_else(|| models.best_lm())
    }
    .ok_or_else(|| anyhow::anyhow!("no language model installed"))?;

    let (dit_model, steps, shift) = match (best_quality, models.dit_sft.first()) {
        (true, Some(sft)) => (sft.clone(), 50, 1.0),
        _ => (
            models
                .dit_turbo
                .first()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no turbo model installed"))?,
            8,
            3.0,
        ),
    };

    // --- stage 1: musical metadata and audio codes --------------------------
    emit_stage(app, job_id, "composing", "Composing the music");

    // Only nudge language through the caption when ACE-Step is writing the
    // words itself. When we supply lyrics, the language is already settled and
    // the caption should stay purely about the music.
    let caption = match &expanded_style {
        Some(style) => style.clone(),
        None if needs_writer && !opts.instrumental => format!(
            "{}. Sung in {}, with {} lyrics.",
            opts.prompt.trim_end_matches(['.', ' ']),
            language_name(&language),
            language_name(&language)
        ),
        None => opts.prompt.clone(),
    };

    let mut req = json!({
        "lm_model": lm_model,
        "synth_model": dit_model,
        "caption": caption,
        "lyrics": lyrics,
        "duration": opts.duration,
        "inference_steps": steps,
        "guidance_scale": 1.0,
        "shift": shift,
        // Upstream defaults to 128 kbps, which is audibly lossy on music.
        // Encoding runs at ~58x realtime, so the higher rate costs nothing.
        "mp3_bitrate": 320,
        // No clipping. The engine's default normalisation scales the 99.999th
        // percentile to full scale and hard-clips everything above it, trading
        // distortion for loudness. Measured on the same song: flat factor 5.42
        // and 21 samples pinned at full scale by default, versus 0.000 and 1
        // with clipping off, at a cost of 2.1 dB. Flattened transients are not
        // worth two decibels in a tool whose output people take elsewhere.
        "peak_clip": 0,
        // The engine's default temperature of 0.85 is far too loose for lyrics
        // and collapses into repetition. Measured over the same prompt, share
        // of distinct lines:
        //   t=0.85  13%   ("I talk talk talk talk talk", 56 lines / ~7 unique)
        //   t=0.65  52%   but 12% of lines were vocalisations
        //   t=0.45  48%   and no vocalisations at all
        // 0.45 gives nearly the best variety with none of the filler.
        "lm_temperature": opts.lyric_variety.unwrap_or(0.45),
    });
    // Don't let the engine rewrite the caption when we already have a good one,
    // or when the user wrote their own lyrics. In both cases someone has been
    // deliberate about what they want, and the engine's rewrite drifts —
    // "a modern jazz song" became "theatrical art-pop" on a track whose lyrics
    // the user had written themselves.
    let keep_caption = expanded_style.is_some() || user_wrote_lyrics;
    req["use_cot_caption"] = json!(opts.embellish && !keep_caption);
    if let Some(b) = opts.bpm.filter(|b| *b > 0) {
        req["bpm"] = json!(b);
    }
    if let Some(k) = opts.keyscale.as_ref().filter(|k| !k.trim().is_empty()) {
        req["keyscale"] = json!(k);
    }
    if let Some(t) = opts.timesignature.as_ref().filter(|t| !t.trim().is_empty()) {
        req["timesignature"] = json!(t);
    }
    req["vocal_language"] = json!(language);
    // -1 means "pick one for me"; the engine echoes back the seed it used, so a
    // track can always be reproduced from its library entry.
    if let Some(s) = opts.seed.filter(|s| *s >= 0) {
        req["seed"] = json!(s);
    }

    // Several takes of the same prompt: same words, different performances.
    let takes = opts.variations.unwrap_or(1).clamp(1, 4);
    if takes > 1 {
        req["lm_batch_size"] = json!(takes);
    }

    // mp3 at 320 is near-transparent and about a tenth the size; wav is there
    // for anyone taking a track into a DAW.
    let format = opts.format.clone().unwrap_or_else(|| "mp3".into());
    req["output_format"] = json!(format);
    let ext = if format.starts_with("wav") { "wav" } else { "mp3" };

    // Voice reference, if one was chosen. The cached latent is preferred, same
    // as everywhere else, because it skips a VAE encode.
    let voice: Option<SynthSources> = opts
        .voice_from
        .as_ref()
        .filter(|id| !id.trim().is_empty())
        .and_then(|id| st.library.lock().unwrap().get(id).ok().flatten())
        .map(|t| {
            let ref_latent = t.latent_path.as_ref().and_then(|p| std::fs::read(p).ok());
            let ref_audio = if ref_latent.is_none() {
                std::fs::read(&t.audio_path).ok()
            } else {
                None
            };
            SynthSources { ref_audio, ref_latent, ..Default::default() }
        });

    let lm_id = ace.submit_lm(&req)?;
    wait_for(ace, &lm_id, app, job_id, "writing", "Writing the song")?;
    let enriched_all = ace.lm_results(&lm_id)?;

    // --- stage 2: render each take -----------------------------------------
    let mut saved: Vec<Track> = Vec::new();
    let total = enriched_all.len();
    for (index, mut enriched) in enriched_all.into_iter().enumerate() {
        let detail = if total > 1 {
            format!("Recording take {} of {}", index + 1, total)
        } else {
            "Recording the audio".to_string()
        };
        emit_stage(app, job_id, "rendering", &detail);

        enriched["output_format"] = json!(format);
        enriched["mp3_bitrate"] = json!(320);
        enriched["peak_clip"] = json!(0);

        let synth_id = match &voice {
            Some(v) => ace.submit_synth_with_source(
                &enriched,
                SynthSources {
                    ref_audio: v.ref_audio.clone(),
                    ref_latent: v.ref_latent.clone(),
                    ..Default::default()
                },
            )?,
            None => ace.submit_synth(&enriched)?,
        };
        wait_for(ace, &synth_id, app, job_id, "rendering", &detail)?;
        let out = ace.synth_result(&synth_id)?;

        let id = uuid::Uuid::new_v4().to_string();
        let lib = st.library.lock().unwrap();
        let audio_path = lib.audio_dir.join(format!("{id}.{ext}"));
        std::fs::write(&audio_path, &out.audio)?;

        let latent_path = out.latent.as_ref().and_then(|l| {
            let p = lib.audio_dir.join(format!("{id}.latent"));
            std::fs::write(&p, l).ok().map(|_| p.display().to_string())
        });

        let base_title = title_from(&opts.prompt);
        let track = Track {
            id,
            title: if total > 1 {
                format!("{base_title} (take {})", index + 1)
            } else {
                base_title
            },
            prompt: opts.prompt.clone(),
            caption: enriched.get("caption").and_then(Value::as_str).unwrap_or_default().to_string(),
            lyrics: enriched.get("lyrics").and_then(Value::as_str).unwrap_or_default().to_string(),
            bpm: enriched.get("bpm").and_then(Value::as_i64),
            keyscale: enriched.get("keyscale").and_then(Value::as_str).map(String::from),
            timesignature: enriched.get("timesignature").and_then(Value::as_str).map(String::from),
            vocal_language: enriched.get("vocal_language").and_then(Value::as_str).map(String::from),
            duration: enriched.get("duration").and_then(Value::as_f64).unwrap_or(opts.duration),
            seed: enriched.get("seed").and_then(Value::as_i64),
            model: dit_model.clone(),
            audio_path: audio_path.display().to_string(),
            latent_path,
            created_at: now_secs(),
            parent_id: None,
            operation: None,
            favorite: false,
            lyricist: lyricist.clone(),
            missing: false,
        };
        lib.insert(&track)?;
        drop(lib);

        // Publish each take as it lands, so the first is playable while the
        // rest are still rendering.
        let _ = app.emit("gen:track", json!({ "job_id": job_id, "track": track }));
        saved.push(track);
    }

    emit_stage(app, job_id, "saving", "Saving to your library");
    Ok(saved)
}

fn wait_for(
    ace: &AceClient,
    id: &str,
    app: &AppHandle,
    job_id: &str,
    stage: &str,
    detail: &str,
) -> anyhow::Result<()> {
    let mut ticks = 0u32;
    loop {
        match ace.poll(id)? {
            JobStatus::Done => return Ok(()),
            JobStatus::Failed => return Err(anyhow::anyhow!("the engine could not finish this song")),
            JobStatus::Cancelled => return Err(anyhow::anyhow!("cancelled")),
            _ => {}
        }
        ticks += 1;
        // Re-emit periodically so the UI can show elapsed time without polling us.
        if ticks % 4 == 0 {
            emit_stage(app, job_id, stage, detail);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// A readable title from the prompt: first few words, trimmed.
fn title_from(prompt: &str) -> String {
    let words: Vec<&str> = prompt.split_whitespace().take(6).collect();
    if words.is_empty() {
        "Untitled".to_string()
    } else {
        let mut t = words.join(" ");
        if let Some(c) = t.chars().next() {
            t.replace_range(0..c.len_utf8(), &c.to_uppercase().to_string());
        }
        t.trim_end_matches(&[',', '.', ';'][..]).to_string()
    }
}

// ----------------------------------------------------------------------- boot

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let audio_dir = dirs::audio_dir()
                .map(|m| m.join("Aria"))
                .unwrap_or_else(|| data_dir.join("audio"));

            let settings_path = data_dir.join("engine.json");
            let settings = load_settings(&settings_path);

            let paths = EnginePaths::discover(app.path().resource_dir().ok().as_deref())
                .map_err(|e| format!("{e}"))?;
            let library = Arc::new(Mutex::new(Library::open(&data_dir, &audio_dir)?));

            // Audio plays over loopback HTTP; see audio_server.rs for why every
            // other transport failed.
            let audio = audio_server::AudioServer::start(Arc::clone(&library))
                .map_err(|e| format!("{e}"))?;
            let audio_base = audio.base_url();
            eprintln!("[audio] serving on {audio_base}");

            app.manage(Arc::new(AppState {
                engine: Mutex::new(Engine::new(paths, settings)),
                library,
                settings_path,
                audio_base,
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            engine_status,
            start_engine,
            generate,
            list_tracks,
            set_favorite,
            rename_track,
            delete_track,
            library_folder,
            languages,
            default_language,
            audio_output_available,
            open_library_folder,
            stem_choices,
            derive_track,
            audio_base_url,
            import_audio,
            setup_info,
            download_models,
            log_ui_error,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Aria");
}

/// Languages the UI offers. ACE-Step supports 50+; these are the common ones,
/// and the field accepts any code the engine knows.
#[tauri::command]
fn languages() -> Vec<(String, String)> {
    [
        ("en", "English"), ("es", "Spanish"), ("fr", "French"), ("de", "German"),
        ("it", "Italian"), ("pt", "Portuguese"), ("nl", "Dutch"), ("pl", "Polish"),
        ("ru", "Russian"), ("uk", "Ukrainian"), ("tr", "Turkish"), ("ar", "Arabic"),
        ("he", "Hebrew"), ("hi", "Hindi"), ("bn", "Bengali"), ("ta", "Tamil"),
        ("ur", "Urdu"), ("fa", "Persian"), ("zh", "Chinese"), ("ja", "Japanese"),
        ("ko", "Korean"), ("vi", "Vietnamese"), ("th", "Thai"), ("id", "Indonesian"),
        ("ms", "Malay"), ("tl", "Filipino"), ("sw", "Swahili"), ("yo", "Yoruba"),
        ("zu", "Zulu"), ("af", "Afrikaans"), ("el", "Greek"), ("sv", "Swedish"),
        ("no", "Norwegian"), ("da", "Danish"), ("fi", "Finnish"), ("cs", "Czech"),
        ("hu", "Hungarian"), ("ro", "Romanian"), ("mi", "Māori"),
    ]
    .iter()
    .map(|(c, n)| (c.to_string(), n.to_string()))
    .collect()
}

#[tauri::command]
fn default_language() -> String {
    default_vocal_language()
}

/// Human-readable name for a language code, for use inside a caption. The model
/// responds to "Sung in Japanese" far better than to a bare "ja".
fn language_name(code: &str) -> String {
    languages()
        .into_iter()
        .find(|(c, _)| c == code)
        .map(|(_, n)| n)
        .unwrap_or_else(|| "English".to_string())
}

/// Open the user's music folder in their file manager.
///
/// Done here rather than through the opener plugin: the plugin call was silently
/// denied, and shelling out to the platform handler is both simpler and easier
/// to report failures from.
#[tauri::command]
fn open_library_folder(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let dir = state.library.lock().unwrap().audio_dir.clone();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";

    std::process::Command::new(program)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {}: {e}", dir.display()))
}

/// Can the webview actually produce sound?
///
/// On Linux, WebKitGTK plays `<audio>` through GStreamer and needs the
/// `autodetect` plugin (`gst-plugins-good`) to find an output. Without it the
/// player controls appear and behave normally but stay completely silent, with
/// nothing in the UI to explain why — the worst possible failure for a music
/// app. Detect it up front and say so plainly.
#[cfg(target_os = "linux")]
#[tauri::command]
fn audio_output_available() -> bool {
    // libgstautodetect.so is what provides autoaudiosink.
    let roots = [
        "/usr/lib/gstreamer-1.0",
        "/usr/lib64/gstreamer-1.0",
        "/usr/lib/x86_64-linux-gnu/gstreamer-1.0",
        "/usr/local/lib/gstreamer-1.0",
    ];
    if let Ok(extra) = std::env::var("GST_PLUGIN_PATH") {
        if std::path::Path::new(&extra).join("libgstautodetect.so").exists() {
            return true;
        }
    }
    roots
        .iter()
        .any(|r| std::path::Path::new(r).join("libgstautodetect.so").exists())
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
fn audio_output_available() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::title_from;

    #[test]
    fn builds_titles_from_prompts() {
        assert_eq!(title_from("warm indie folk with guitar"), "Warm indie folk with guitar");
        assert_eq!(title_from(""), "Untitled");
        assert_eq!(
            title_from("dreamy lo-fi hip hop, mellow rhodes piano, soft vinyl"),
            "Dreamy lo-fi hip hop, mellow rhodes"
        );
    }

    #[test]
    fn derives_language_from_locale() {
        use super::parse_locale;
        assert_eq!(parse_locale("en_NZ.UTF-8"), "en");
        assert_eq!(parse_locale("fr_FR@euro"), "fr");
        assert_eq!(parse_locale("pt_BR"), "pt");
        assert_eq!(parse_locale("ja"), "ja");
        // Anything unusable falls back to English rather than to "random".
        assert_eq!(parse_locale("C"), "en");
        assert_eq!(parse_locale("POSIX"), "en");
        assert_eq!(parse_locale(""), "en");
    }
}
