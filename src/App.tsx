import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import Create from "./components/Create";
import LibraryView from "./components/Library";
import PlayerBar from "./components/Player";
import Setup from "./components/Setup";
import {
  coalesceAnnouncement,
  deriveFinished,
  generationFinished,
  generationProgress,
  listKeyIntent,
  loadLargeText,
  moveIndex,
  saveLargeText,
  useAnnounce,
} from "./a11y";
import { api, loadAudioBase, onEvent, openFolder, pickAudioFile } from "./api";
import { usePlayer } from "./player";
import { isDesktop } from "./preview";
import type { EngineStatus, Persona, Playlist, Track, VideoSupport } from "./types";

type Tab = "create" | "library";

export default function App() {
  const [tab, setTab] = useState<Tab>("create");
  const [tracks, setTracks] = useState<Track[]>([]);
  const [playlists, setPlaylists] = useState<Playlist[]>([]);
  const [personas, setPersonas] = useState<Persona[]>([]);
  // Checked once at launch: it shells out to ffmpeg, and the answer can't
  // change while the app is open.
  const [videoSupport, setVideoSupport] = useState<VideoSupport | null>(null);
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [largeText, setLargeText] = useState(loadLargeText);
  const announce = useAnnounce();
  const tabRefs = useRef<Record<Tab, HTMLButtonElement | null>>({
    create: null,
    library: null,
  });
  // After a song lands we move keyboard focus to My songs, because Create
  // unmounts and the button they just pressed is gone.
  const focusLibrary = useRef(false);
  const lastProgress = useRef<string | null>(null);
  const [canPlayAudio, setCanPlayAudio] = useState(true);
  // A derived track is being made; the library disables its controls meanwhile.
  const [deriving, setDeriving] = useState(false);
  // Work in progress, shown in the library. Aria jumps there as soon as the
  // first take is playable, which used to leave the remaining takes rendering
  // out of sight — it looked like only one had been made.
  const [working, setWorking] = useState<string | null>(null);
  // Null until we've checked; true when models still need downloading. The
  // whole app is unusable without them, so setup takes over the window.
  const [needsSetup, setNeedsSetup] = useState<boolean | null>(null);
  const [importing, setImporting] = useState(false);
  // One player for the whole app; the library only ever hands it a queue.
  const player = usePlayer();

  // Bringing in your own recording unlocks every derived operation on it —
  // stems from a demo, a restyle of a voice memo. Suno's free tier has no
  // equivalent.
  const importSong = useCallback(async () => {
    const path = await pickAudioFile();
    if (!path) return;
    setImporting(true);
    try {
      await api.importAudio(path);
      setTab("library");
    } catch (e) {
      setBootError(`Could not bring that song in: ${e}`);
      setImporting(false);
    }
  }, []);

  const refreshTracks = useCallback(async () => {
    try {
      setTracks(await api.listTracks());
    } catch (e) {
      console.error(e);
    }
  }, []);

  const refreshPlaylists = useCallback(async () => {
    try {
      setPlaylists(await api.listPlaylists());
    } catch (e) {
      console.error(e);
    }
  }, []);

  const refreshPersonas = useCallback(async () => {
    try {
      setPersonas(await api.listPersonas());
    } catch (e) {
      console.error(e);
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.engineStatus());
    } catch (e) {
      setBootError(String(e));
    }
  }, []);

  // Start the engine once at launch. It takes a few seconds, so we do it up
  // front rather than making the first song pay for it.
  useEffect(() => {
    (async () => {
      // Must run before the library renders, or track URLs come out empty.
      await loadAudioBase();

      // Don't start the engine before the models exist — it would fail in a
      // way that reads like a broken app rather than an unfinished setup.
      try {
        const setup = await api.setupInfo();
        setNeedsSetup(!setup.ready);
        if (!setup.ready) return;
      } catch {
        setNeedsSetup(false);
      }

      await refreshStatus();
      await refreshTracks();
      await refreshPlaylists();
      await refreshPersonas();
      api.videoSupport().then(setVideoSupport).catch(() => {});
      setCanPlayAudio(await api.audioOutputAvailable());
      // Same reasoning as the models check above: starting an engine that was
      // never built fails in a way that looks like a crash. The footer says so
      // plainly instead.
      const st = await api.engineStatus().catch(() => null);
      if (st?.engine_installed !== false) {
        try {
          await api.startEngine();
        } catch (e) {
          setBootError(String(e));
        }
      }
      await refreshStatus();
    })();
  }, [refreshStatus, refreshTracks, refreshPlaylists, refreshPersonas]);

  useEffect(() => {
    document.body.classList.toggle("text-large", largeText);
    saveLargeText(largeText);
  }, [largeText]);

  // Derived tracks finish on the same events as generation. Refresh the list
  // and re-enable the library's controls when one lands.
  useEffect(() => {
    const offs = [
      onEvent<{ stage: string; detail: string }>("gen:stage", (p) => {
        setWorking(p.detail || null);
        const next = coalesceAnnouncement(
          lastProgress.current,
          generationProgress(p.stage, p.detail),
        );
        if (next) {
          lastProgress.current = next;
          announce(next);
        }
      }),
      onEvent<{ track: Track }>("gen:track", () => {
        refreshTracks();
      }),
      onEvent<{ track: Track }>("gen:done", (p) => {
        setDeriving(false);
        setImporting(false);
        setWorking(null);
        lastProgress.current = null;
        refreshTracks();
        announce(p.track ? generationFinished(p.track.title) : deriveFinished());
      }),
      onEvent<{ message: string }>("gen:error", (p) => {
        setDeriving(false);
        setImporting(false);
        setWorking(null);
        setBootError(p.message);
      }),
    ];
    return () => {
      offs.forEach((o) => o.then((f) => f()));
    };
  }, [refreshTracks, announce]);

  const onCreated = useCallback(
    (t: Track) => {
      setTracks((prev) => [t, ...prev]);
      focusLibrary.current = true;
      setTab("library");
    },
    [],
  );

  useEffect(() => {
    if (tab !== "library" || !focusLibrary.current) return;
    focusLibrary.current = false;
    document.getElementById("panel-library")?.focus();
  }, [tab, tracks]);

  const tabs: Tab[] = ["create", "library"];

  function onTabsKeyDown(e: KeyboardEvent) {
    const intent = listKeyIntent(e.key);
    if (!intent) return;
    e.preventDefault();
    const next = tabs[moveIndex(tabs.indexOf(tab), tabs.length, intent)];
    setTab(next);
    tabRefs.current[next]?.focus();
  }

  const ready = status?.state === "ready";
  const engineMissing = status?.engine_installed === false;
  const modelsMissing = status && !status.models_complete;

  return (
    <>
      <a className="skip-link" href="#main">Skip to main content</a>

      <div className="app">
        <header className="masthead">
          <h1 className="wordmark">
            Aria<span className="dot">.</span>
          </h1>
          <p className="tagline">Your music. Your machine. No limits.</p>
          <div className="masthead-actions">
            <button
              type="button"
              className="btn btn-icon"
              onClick={importSong}
              disabled={importing || !ready}
            >
              {importing ? "Bringing it in…" : "Add my own song"}
            </button>
            <button
              type="button"
              className="btn btn-icon"
              onClick={() => setLargeText((v) => !v)}
              aria-pressed={largeText}
            >
              {largeText ? "Normal text" : "Larger text"}
            </button>
          </div>
        </header>

        {needsSetup && (
          <main className="panel" id="main">
            <Setup
              onReady={async () => {
                setNeedsSetup(false);
                await refreshStatus();
                await refreshTracks();
                await refreshPlaylists();
                await refreshPersonas();
                try {
                  await api.startEngine();
                } catch (e) {
                  setBootError(String(e));
                }
                await refreshStatus();
              }}
            />
          </main>
        )}

        {!needsSetup && (
        <>
        <div className="tabs" role="tablist" aria-label="Sections" onKeyDown={onTabsKeyDown}>
          <button
            type="button"
            role="tab"
            id="tab-create"
            className="tab"
            ref={(el) => { tabRefs.current.create = el; }}
            aria-selected={tab === "create"}
            aria-controls="panel-create"
            tabIndex={tab === "create" ? 0 : -1}
            onClick={() => setTab("create")}
          >
            Create
          </button>
          <button
            type="button"
            role="tab"
            id="tab-library"
            className="tab"
            ref={(el) => { tabRefs.current.library = el; }}
            aria-selected={tab === "library"}
            aria-controls="panel-library"
            tabIndex={tab === "library" ? 0 : -1}
            onClick={() => setTab("library")}
          >
            My songs{tracks.length > 0 ? ` (${tracks.length})` : ""}
          </button>
        </div>

        <main className="panel" id="main">
          {bootError && (
            <div className="notice notice-err" role="alert">
              <p>
                <strong>Aria couldn't start its engine</strong>
                {bootError}
              </p>
            </div>
          )}

          {!canPlayAudio && (
            <div className="notice notice-warn" role="status">
              <p>
                <strong>Songs will play silently until one package is installed</strong>
                Your system is missing the GStreamer plugin this app needs to send
                sound to your speakers. Aria can still make music, and the files in
                your music folder are fine — they just won't play inside this window.
                Install <code>gst-plugins-good</code>, then restart Aria.
              </p>
            </div>
          )}

          {modelsMissing && (
            <div className="notice notice-warn" role="status">
              <p>
                <strong>Some model files are missing</strong>
                Aria needs a language model, a text encoder, a sound model and a
                decoder before it can make music.
              </p>
            </div>
          )}

          {tab === "create" ? (
            <div role="tabpanel" id="panel-create" aria-labelledby="tab-create">
              <Create
                onCreated={onCreated}
                engineReady={!!ready}
                tracks={tracks}
                personas={personas}
                onPersonasChanged={refreshPersonas}
                lyricWriter={status?.lyric_writer ?? null}
              />
            </div>
          ) : (
            <div
              role="tabpanel"
              id="panel-library"
              aria-labelledby="tab-library"
              tabIndex={-1}
            >
              {/* Visual only. Stage and completion are spoken by the window-level
                  live region, which stays mounted when this panel is showing. */}
              {working && (
                <div className="working-strip">
                  <span className="pulse" aria-hidden="true" />
                  <span>{working}</span>
                  <span className="working-note">
                    Songs appear here as each one finishes.
                  </span>
                </div>
              )}
              <LibraryView
                tracks={tracks}
                playlists={playlists}
                onChanged={refreshTracks}
                onPlaylistsChanged={refreshPlaylists}
                supportsStems={!!status?.supports_stems}
                busy={deriving}
                onDeriveStarted={() => setDeriving(true)}
                player={player}
                onPersonaSaved={refreshPersonas}
                videoSupport={videoSupport}
              />
            </div>
          )}
        </main>
        </>
        )}

        <PlayerBar player={player} />

        <footer className="footer">
          <span role="status">
            <span
              className={
                "status-dot " +
                (ready ? "status-ready" : status?.state === "starting" ? "status-busy" : "status-stopped")
              }
              aria-hidden="true"
            />
            {engineMissing
              ? "No engine installed — your songs still play"
              : ready
                ? "Engine ready"
                : status?.state === "starting"
                  ? "Engine starting…"
                  : "Engine stopped"}
          </span>

          {status?.cpu_fallback && <span>Using CPU rendering</span>}

          <button
            type="button"
            className="btn btn-icon"
            onClick={async () => {
              try {
                await openFolder();
              } catch (e) {
                setBootError(`Could not open your music folder: ${e}`);
              }
            }}
          >
            Open my music folder
          </button>

          {!isDesktop && <span>Design preview — not the real app</span>}

          <span style={{ marginLeft: "auto" }}>
            Everything you make is yours.
          </span>
        </footer>
      </div>
    </>
  );
}
