//! Supervisor for the acestep.cpp inference server.
//!
//! The engine is an external process (`ace-server`) that we own the lifecycle of.
//! Two hard-won facts from Phase 0 shape this module:
//!
//! 1. On AMD/RADV, a VAE decode tile that runs longer than amdgpu's compute-ring
//!    watchdog kills the GPU context. The kernel logs `ring comp_* timeout` and the
//!    server dies with `vk::DeviceLostError`. Upstream's default `--vae-chunk 1024`
//!    reliably triggers this on Navi 23; even 256 proved marginal.
//!
//! 2. Because the failure aborts the process, "handle the error" means *supervise a
//!    crash*, not catch an exception. So: detect death, shrink the chunk, restart,
//!    and retry the job — without ever showing the user a driver crash.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// Chunk sizes to fall back through, largest first.
/// Measured on an RX 6650 XT (engine/bench-vae-chunk.sh): a 60s song decodes in
/// 9-10s at *every* size from 64 to 256. Chunk size is essentially free, so we
/// trade none of it for reliability.
pub const CHUNK_LADDER: &[u32] = &[256, 192, 128, 64];

/// Where we start on a GPU we've never seen. Far below upstream's 1024: a
/// first-run driver crash loses the user permanently, and it costs us nothing.
pub const DEFAULT_CHUNK: u32 = 128;

const HOST: &str = "127.0.0.1";
const PORT: u16 = 8422;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSettings {
    /// VAE latent frames per tile. Persisted per machine once we find one that holds.
    pub vae_chunk: u32,
    /// Set once a full generation has succeeded at `vae_chunk`, so we stop probing.
    pub vae_chunk_verified: bool,
    /// Force CPU decode. Last-resort fallback if every chunk size loses the device.
    pub cpu_fallback: bool,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            vae_chunk: DEFAULT_CHUNK,
            vae_chunk_verified: false,
            cpu_fallback: false,
        }
    }
}

/// Filesystem locations the engine needs. Resolved once at startup.
#[derive(Debug, Clone)]
pub struct EnginePaths {
    pub server_bin: PathBuf,
    pub models_dir: PathBuf,
}

impl EnginePaths {
    /// Look for a built engine in the places it can legitimately live: next to the
    /// app (installed), or in the repo's `engine/` tree (development).
    pub fn discover(app_dir: Option<&Path>) -> Result<Self> {
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(d) = app_dir {
            roots.push(d.to_path_buf());
            roots.push(d.join("engine"));
        }
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd.join("engine/acestep.cpp"));
            roots.push(cwd.join("../engine/acestep.cpp"));
        }

        for root in &roots {
            // build-static first: that's what packaging produces, and the only
            // one that runs without ggml shared libraries sitting beside it.
            let bin = root.join("build-static/ace-server");
            let shared = root.join("build/ace-server");
            let alt = root.join("ace-server");
            let models = root.join("models");
            for candidate in [bin, shared, alt] {
                if candidate.is_file() && models.is_dir() {
                    return Ok(Self {
                        server_bin: candidate,
                        models_dir: models,
                    });
                }
            }
        }
        Err(anyhow!(
            "Could not find the Aria engine. Expected ace-server and a models/ \
             directory under engine/acestep.cpp."
        ))
    }

    /// Which model files are actually present. Drives what the UI offers, so we
    /// never show a feature the user's install can't perform.
    pub fn available_models(&self) -> Result<AvailableModels> {
        let mut m = AvailableModels::default();
        for entry in std::fs::read_dir(&self.models_dir)? {
            let name = entry?.file_name().to_string_lossy().to_string();
            if !name.ends_with(".gguf") {
                continue;
            }
            if name.contains("-lm-") {
                m.lm.push(name);
            } else if name.starts_with("Qwen3-Embedding") {
                m.embedding.push(name);
            } else if name.starts_with("vae-") {
                m.vae.push(name);
            } else if name.contains("turbo") {
                m.dit_turbo.push(name);
            } else if name.contains("sft") || name.contains("base") {
                m.dit_sft.push(name);
            }
        }
        Ok(m)
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct AvailableModels {
    pub lm: Vec<String>,
    pub embedding: Vec<String>,
    pub vae: Vec<String>,
    pub dit_turbo: Vec<String>,
    pub dit_sft: Vec<String>,
}

impl AvailableModels {
    fn lm_rank(name: &str) -> u8 {
        if name.contains("-4B-") {
            3
        } else if name.contains("-1.7B-") {
            2
        } else if name.contains("-0.6B-") {
            1
        } else {
            0
        }
    }

    /// Largest installed language model.
    ///
    /// Size buys three things we measured over three runs each on the same
    /// prompt: language reliability (4B 3/3 English, 1.7B 2/3 — one run came
    /// back Dutch), subject adherence (4B mentioned the prompt's subject in
    /// 2/3, the 1.7B in 0/3), and basic competence (one 1.7B run sang only
    /// "Hmmm"). It costs about 20s more per song.
    pub fn best_lm(&self) -> Option<String> {
        self.lm.iter().max_by_key(|n| Self::lm_rank(n)).cloned()
    }

    /// Smallest installed language model, for the fast path.
    pub fn fastest_lm(&self) -> Option<String> {
        self.lm.iter().min_by_key(|n| Self::lm_rank(n)).cloned()
    }

    pub fn is_complete(&self) -> bool {
        !self.lm.is_empty()
            && !self.embedding.is_empty()
            && !self.vae.is_empty()
            && !self.dit_turbo.is_empty()
    }
    /// Stem separation, lego and complete need the SFT/base DiT; turbo can't do them.
    pub fn supports_stems(&self) -> bool {
        !self.dit_sft.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineState {
    Stopped,
    Starting,
    Ready,
    /// Died unexpectedly — almost always a lost GPU device mid-decode.
    Crashed,
}

pub struct Engine {
    pub paths: EnginePaths,
    pub settings: EngineSettings,
    state: Arc<Mutex<EngineState>>,
    child: Option<Child>,
    /// Tail of engine stderr, so a crash can be explained rather than just reported.
    log_tail: Arc<Mutex<Vec<String>>>,
}

impl Engine {
    pub fn new(paths: EnginePaths, settings: EngineSettings) -> Self {
        Self {
            paths,
            settings,
            state: Arc::new(Mutex::new(EngineState::Stopped)),
            child: None,
            log_tail: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{HOST}:{PORT}")
    }

    pub fn state(&self) -> EngineState {
        *self.state.lock().unwrap()
    }

    #[allow(dead_code)]
    pub fn recent_log(&self) -> Vec<String> {
        self.log_tail.lock().unwrap().clone()
    }

    /// Kill an engine left behind by a previous run.
    ///
    /// Exit handlers cover a clean quit and PR_SET_PDEATHSIG covers a parent
    /// that dies while the spawning thread is still alive, but neither survives
    /// a SIGKILL or a hard crash — and an orphan holds several GB of VRAM and
    /// our port. Reclaiming at startup is the one check that catches every case,
    /// because it runs regardless of how the last session ended.
    ///
    /// Deliberately narrow: only processes whose command line names *our*
    /// binary and *our* port, and never the current process.
    #[cfg(target_os = "linux")]
    fn reclaim_orphan(&self) {
        let me = std::process::id();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            if pid == me {
                continue;
            }
            let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            // /proc cmdline is NUL-separated.
            let cmdline = String::from_utf8_lossy(&raw).replace('\0', " ");
            let ours = cmdline.contains("ace-server")
                && cmdline.contains(&format!("--port {PORT}"))
                && cmdline.contains(&self.paths.models_dir.display().to_string());
            if ours {
                eprintln!("[engine] reclaiming orphaned engine (pid {pid}) from a previous run");
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGTERM);
                }
                // Give it a moment to release the GPU before we ask for it.
                std::thread::sleep(Duration::from_millis(800));
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn reclaim_orphan(&self) {}

    /// Spawn the server and block until it answers /health.
    pub fn start(&mut self) -> Result<()> {
        if matches!(self.state(), EngineState::Ready | EngineState::Starting) {
            return Ok(());
        }
        // Before binding our port, take back anything a previous run stranded.
        self.reclaim_orphan();
        *self.state.lock().unwrap() = EngineState::Starting;
        self.log_tail.lock().unwrap().clear();

        let overlap = (self.settings.vae_chunk / 8).max(8);
        let mut cmd = Command::new(&self.paths.server_bin);
        cmd.arg("--models")
            .arg(&self.paths.models_dir)
            .arg("--host")
            .arg(HOST)
            .arg("--port")
            .arg(PORT.to_string())
            .arg("--vae-chunk")
            .arg(self.settings.vae_chunk.to_string())
            .arg("--vae-overlap")
            .arg(overlap.to_string())
            // Upstream caps the language model batch at 1, which silently
            // reduces a request for several takes to one — verified: the same
            // request returned 1 take at the default and 2 once raised.
            .arg("--max-batch")
            .arg("4")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // NOTE: deliberately no `--keep-loaded`.
        //
        // Keeping the LM, DiT and VAE all resident is what actually caused our
        // GPU hangs — not the chunk size. With everything resident the server
        // lost the device at chunk 256, while the CLI (which unloads between
        // stages) completed at every size we tried. Reloading costs a few
        // seconds per request; a wedged graphics driver costs the user.

        if self.settings.cpu_fallback {
            cmd.env("GGML_BACKEND", "cpu");
        }

        // Tie the engine's life to ours, so a force-quit can't strand a process
        // holding several GB of VRAM with no UI left to free it.
        //
        // PR_SET_PDEATHSIG fires when the parent *THREAD* exits, not the parent
        // process. That is the whole trap: we spawn from a `spawn_blocking`
        // worker, and when tokio retires that idle worker the kernel promptly
        // SIGTERMs the engine. Measured: the engine went zombie about 30
        // seconds after every startup, and only survived at all because
        // `run_generation` notices the death and restarts it — so every song
        // silently paid for a fresh multi-gigabyte model load.
        //
        // The signal is therefore armed against the thread that owns the
        // engine's whole lifetime, and only if that thread is still the one
        // that spawned it. Everything else is handled by `Drop` and by the
        // explicit shutdown on app exit.
        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::process::CommandExt;
            // The tid of the thread doing the spawn, captured before the fork.
            let spawning_tid = libc::syscall(libc::SYS_gettid) as libc::pid_t;
            cmd.pre_exec(move || {
                // Only meaningful when the spawning thread is the process's
                // main thread; otherwise that thread can retire long before the
                // app does and take the engine with it.
                if spawning_tid == libc::getpid() {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                // Put the child in its own process group so a Ctrl-C to the
                // terminal doesn't race our own shutdown handling.
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to launch engine at {}",
                self.paths.server_bin.display()
            )
        })?;

        // Drain stderr into a ring buffer. Without this the pipe fills and the
        // engine blocks mid-generation.
        if let Some(stderr) = child.stderr.take() {
            let tail = Arc::clone(&self.log_tail);
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let mut t = tail.lock().unwrap();
                    t.push(line);
                    if t.len() > 200 {
                        t.remove(0);
                    }
                }
            });
        }
        if let Some(stdout) = child.stdout.take() {
            let tail = Arc::clone(&self.log_tail);
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let mut t = tail.lock().unwrap();
                    t.push(line);
                    if t.len() > 200 {
                        t.remove(0);
                    }
                }
            });
        }

        self.child = Some(child);

        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let url = format!("{}/health", self.base_url());
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        while Instant::now() < deadline {
            if let Some(c) = self.child.as_mut() {
                if let Ok(Some(status)) = c.try_wait() {
                    *self.state.lock().unwrap() = EngineState::Crashed;
                    return Err(anyhow!(
                        "engine exited during startup ({status}): {}",
                        self.explain_failure()
                    ));
                }
            }
            if client
                .get(&url)
                .send()
                .map(|r| r.status().is_success())
                .unwrap_or(false)
            {
                *self.state.lock().unwrap() = EngineState::Ready;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(400));
        }

        *self.state.lock().unwrap() = EngineState::Crashed;
        Err(anyhow!(
            "engine did not become ready within {STARTUP_TIMEOUT:?}"
        ))
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        *self.state.lock().unwrap() = EngineState::Stopped;
    }

    /// True if the process is gone. A lost GPU device aborts the engine, so
    /// liveness is how we detect it.
    pub fn has_died(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(Some(_))),
            None => true,
        }
    }

    /// Did the last crash look like a lost GPU device?
    pub fn crashed_on_device_lost(&self) -> bool {
        self.log_tail.lock().unwrap().iter().any(|l| {
            l.contains("DeviceLost")
                || l.contains("context is lost")
                || l.contains("ErrorDeviceLost")
        })
    }

    /// Plain-language cause for the UI. Users should never see a Vulkan backtrace.
    pub fn explain_failure(&self) -> String {
        if self.crashed_on_device_lost() {
            "the graphics driver interrupted a long decode step".to_string()
        } else {
            self.log_tail
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|l| l.contains("error") || l.contains("Error"))
                .cloned()
                .unwrap_or_else(|| "the engine stopped unexpectedly".to_string())
        }
    }

    /// Step down to the next smaller VAE chunk. Returns false when we've run out,
    /// at which point the caller should switch to CPU decode.
    pub fn reduce_chunk(&mut self) -> bool {
        let next = CHUNK_LADDER
            .iter()
            .copied()
            .find(|&c| c < self.settings.vae_chunk);
        match next {
            Some(c) => {
                self.settings.vae_chunk = c;
                self.settings.vae_chunk_verified = false;
                true
            }
            None => {
                self.settings.cpu_fallback = true;
                false
            }
        }
    }

    /// Record that this chunk size completed a real generation, so later runs
    /// start here instead of re-probing.
    pub fn mark_chunk_verified(&mut self) {
        self.settings.vae_chunk_verified = true;
    }

    pub fn restart(&mut self) -> Result<()> {
        self.stop();
        std::thread::sleep(Duration::from_secs(3)); // let the driver settle after a ring reset
        self.start()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}
