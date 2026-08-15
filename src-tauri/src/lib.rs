//! Aria — free, unlimited, local AI music creation.

mod client;
mod engine;
mod library;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use client::{AceClient, JobStatus};
use engine::{AvailableModels, Engine, EnginePaths, EngineSettings, EngineState};
use library::{Library, Track};

/// How many times we'll shrink the VAE chunk and retry after a GPU device loss
/// before giving up on the GPU and decoding on the CPU.
const MAX_DEVICE_LOST_RETRIES: usize = 3;

pub struct AppState {
    engine: Mutex<Engine>,
    library: Mutex<Library>,
    settings_path: PathBuf,
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
    /// When false, the model rewrites the caption and may override the user's
    /// stated intent. Studio mode sets this true to keep their words.
    #[serde(default)]
    pub lock_prompt: bool,
    #[serde(default)]
    pub bpm: Option<i64>,
    #[serde(default)]
    pub keyscale: Option<String>,
    #[serde(default)]
    pub vocal_language: Option<String>,
    #[serde(default)]
    pub seed: Option<i64>,
}

fn default_duration() -> f64 {
    60.0
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

#[tauri::command]
fn engine_status(state: State<'_, Arc<AppState>>) -> Result<EngineStatus, String> {
    let eng = state.engine.lock().unwrap();
    let models = eng.paths.available_models().unwrap_or_default();
    Ok(EngineStatus {
        state: eng.state(),
        models_complete: models.is_complete(),
        supports_stems: models.supports_stems(),
        models,
        vae_chunk: eng.settings.vae_chunk,
        cpu_fallback: eng.settings.cpu_fallback,
    })
}

#[tauri::command]
fn start_engine(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut eng = state.engine.lock().unwrap();
    eng.start().map_err(|e| e.to_string())
}

#[tauri::command]
fn list_tracks(state: State<'_, Arc<AppState>>, limit: Option<i64>) -> Result<Vec<Track>, String> {
    state
        .library
        .lock()
        .unwrap()
        .list(limit.unwrap_or(500))
        .map_err(|e| e.to_string())
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
            Ok(track) => {
                {
                    let mut eng = st.engine.lock().unwrap();
                    eng.mark_chunk_verified();
                    save_settings(&st.settings_path, &eng.settings);
                }
                let _ = app.emit("gen:done", json!({ "job_id": job_id, "track": track }));
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
) -> anyhow::Result<Track> {
    let models = st.engine.lock().unwrap().paths.available_models()?;
    let lm_model = models
        .lm
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no language model installed"))?;
    let dit_model = models
        .dit_turbo
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no turbo model installed"))?;

    // --- stage 1: lyrics, musical metadata, audio codes ---------------------
    emit_stage(app, job_id, "writing", "Writing the song");

    let lyrics = if opts.instrumental { "[Instrumental]".to_string() } else { opts.lyrics.clone() };

    let mut req = json!({
        "lm_model": lm_model,
        "synth_model": dit_model,
        "caption": opts.prompt,
        "lyrics": lyrics,
        "duration": opts.duration,
        "inference_steps": 8,
        "guidance_scale": 1.0,
        "shift": 3.0,
    });
    // Keep the user's own words when they've asked us to.
    if opts.lock_prompt {
        req["use_cot_caption"] = json!(false);
    }
    if let Some(b) = opts.bpm {
        req["bpm"] = json!(b);
    }
    if let Some(k) = &opts.keyscale {
        req["keyscale"] = json!(k);
    }
    // Always send a language. Omitting it lets the model choose one at random.
    req["vocal_language"] = json!(opts
        .vocal_language
        .clone()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(default_vocal_language));
    if let Some(s) = opts.seed {
        req["seed"] = json!(s);
    }

    let lm_id = ace.submit_lm(&req)?;
    wait_for(ace, &lm_id, app, job_id, "writing", "Writing the song")?;
    let enriched: Value = ace.lm_result(&lm_id)?;

    // --- stage 2: render audio ---------------------------------------------
    emit_stage(app, job_id, "rendering", "Recording the audio");
    let synth_id = ace.submit_synth(&enriched)?;
    wait_for(ace, &synth_id, app, job_id, "rendering", "Recording the audio")?;
    let out = ace.synth_result(&synth_id)?;

    // --- save ---------------------------------------------------------------
    emit_stage(app, job_id, "saving", "Saving to your library");
    let id = uuid::Uuid::new_v4().to_string();
    let lib = st.library.lock().unwrap();
    let audio_path = lib.audio_dir.join(format!("{id}.mp3"));
    std::fs::write(&audio_path, &out.audio)?;

    let latent_path = out.latent.as_ref().and_then(|l| {
        let p = lib.audio_dir.join(format!("{id}.latent"));
        std::fs::write(&p, l).ok().map(|_| p.display().to_string())
    });

    let track = Track {
        id: id.clone(),
        title: title_from(&opts.prompt),
        prompt: opts.prompt.clone(),
        caption: enriched.get("caption").and_then(Value::as_str).unwrap_or_default().to_string(),
        lyrics: enriched.get("lyrics").and_then(Value::as_str).unwrap_or_default().to_string(),
        bpm: enriched.get("bpm").and_then(Value::as_i64),
        keyscale: enriched.get("keyscale").and_then(Value::as_str).map(String::from),
        timesignature: enriched.get("timesignature").and_then(Value::as_str).map(String::from),
        vocal_language: enriched.get("vocal_language").and_then(Value::as_str).map(String::from),
        duration: enriched.get("duration").and_then(Value::as_f64).unwrap_or(opts.duration),
        seed: enriched.get("seed").and_then(Value::as_i64),
        model: dit_model,
        audio_path: audio_path.display().to_string(),
        latent_path,
        created_at: now_secs(),
        parent_id: None,
        operation: None,
        favorite: false,
    };
    lib.insert(&track)?;
    Ok(track)
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
            let library = Library::open(&data_dir, &audio_dir)?;

            app.manage(Arc::new(AppState {
                engine: Mutex::new(Engine::new(paths, settings)),
                library: Mutex::new(library),
                settings_path,
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
