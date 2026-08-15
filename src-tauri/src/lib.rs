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

    // Quality picks a *pair* of models, because both halves matter and both
    // cost time. Roughly, on a 60s song:
    //   best — largest LM + SFT diffusion at 50 steps  (~55s)
    //   fast — smallest LM + turbo diffusion at 8 steps (~23s)
    // Best is the default: unlimited local generation means the only thing a
    // slower setting spends is patience, and the whole point is that nobody is
    // metering it.
    let best_quality = opts.quality.as_deref() != Some("fast");

    let lm_model = if best_quality { models.best_lm() } else { models.fastest_lm() }
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

    // --- stage 1: lyrics, musical metadata, audio codes ---------------------
    emit_stage(app, job_id, "writing", "Writing the song");

    let lyrics = if opts.instrumental { "[Instrumental]".to_string() } else { opts.lyrics.clone() };

    // Nudge the language through the caption. The `vocal_language` field is
    // echoed back but does not steer lyric writing — the library recorded "en"
    // on a song whose lyrics were Norwegian. Naming it in the caption is not
    // perfectly reliable either, but it's the only lever that works at all
    // without disabling expansion (which costs the vocals themselves).
    let language = opts
        .vocal_language
        .clone()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(default_vocal_language);
    let caption = if opts.instrumental {
        opts.prompt.clone()
    } else {
        format!("{}. Sung in {}, with {} lyrics.",
            opts.prompt.trim_end_matches(['.', ' ']),
            language_name(&language),
            language_name(&language))
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
    });
    // See `embellish`: leaving expansion on lets the model's CoT override the
    // requested language, so it stays off unless explicitly asked for.
    req["use_cot_caption"] = json!(opts.embellish);
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
        .register_uri_scheme_protocol("aria", |ctx, request| {
            serve_track(ctx.app_handle(), request)
        })
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
            audio_output_available,
            open_library_folder,
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

/// Serve a track over our own `aria://` URI scheme.
///
/// Third approach to playback, after the asset protocol and blob URLs both
/// failed on WebKitGTK. Those failures were opaque — the player appeared, then
/// erroring on play with nothing in the app log.
///
/// A scheme we implement ourselves has no scope pattern to satisfy and no
/// CSP interaction, and WebKit's media stack is happiest with an ordinary
/// HTTP-shaped response. Range requests are handled so the scrubber works;
/// without `Accept-Ranges` WebKit will refuse to seek and may refuse to start.
fn serve_track(
    app: &AppHandle,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::{header, status::StatusCode, Response};

    let not_found = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap()
    };

    // aria://localhost/track/<id>
    let id = match request.uri().path().strip_prefix("/track/") {
        Some(i) if !i.is_empty() => i.to_string(),
        _ => return not_found(),
    };

    let state = app.state::<Arc<AppState>>();
    let track = match state.library.lock().unwrap().get(&id) {
        Ok(Some(t)) => t,
        _ => return not_found(),
    };
    let bytes = match std::fs::read(&track.audio_path) {
        Ok(b) => b,
        Err(_) => return not_found(),
    };
    let total = bytes.len() as u64;

    // Honour a single-range request; that is all a media element issues.
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| {
            let (a, b) = v.split_once('-')?;
            let start: u64 = a.parse().ok()?;
            let end: u64 = if b.is_empty() {
                total.saturating_sub(1)
            } else {
                b.parse().ok()?
            };
            (start <= end && start < total).then_some((start, end.min(total - 1)))
        });

    match range {
        Some((start, end)) => {
            let slice = bytes[start as usize..=end as usize].to_vec();
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, "audio/mpeg")
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"))
                .header(header::CONTENT_LENGTH, slice.len())
                .header(header::CACHE_CONTROL, "no-store")
                .body(slice)
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "audio/mpeg")
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, total)
            .header(header::CACHE_CONTROL, "no-store")
            .body(bytes)
            .unwrap(),
    }
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
