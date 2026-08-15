import { useCallback, useEffect, useState } from "react";
import Create from "./components/Create";
import LibraryView from "./components/Library";
import { api, loadAudioBase, onEvent, openFolder } from "./api";
import { isDesktop } from "./preview";
import type { EngineStatus, Track } from "./types";

type Tab = "create" | "library";

export default function App() {
  const [tab, setTab] = useState<Tab>("create");
  const [tracks, setTracks] = useState<Track[]>([]);
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [largeText, setLargeText] = useState(false);
  const [canPlayAudio, setCanPlayAudio] = useState(true);
  // A derived track is being made; the library disables its controls meanwhile.
  const [deriving, setDeriving] = useState(false);

  const refreshTracks = useCallback(async () => {
    try {
      setTracks(await api.listTracks());
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
      await refreshStatus();
      await refreshTracks();
      setCanPlayAudio(await api.audioOutputAvailable());
      try {
        await api.startEngine();
      } catch (e) {
        setBootError(String(e));
      }
      await refreshStatus();
    })();
  }, [refreshStatus, refreshTracks]);

  useEffect(() => {
    document.body.classList.toggle("text-large", largeText);
  }, [largeText]);

  // Derived tracks finish on the same events as generation. Refresh the list
  // and re-enable the library's controls when one lands.
  useEffect(() => {
    const offs = [
      onEvent<{ track: Track }>("gen:done", () => {
        setDeriving(false);
        refreshTracks();
      }),
      onEvent<{ message: string }>("gen:error", (p) => {
        setDeriving(false);
        setBootError(p.message);
      }),
    ];
    return () => {
      offs.forEach((o) => o.then((f) => f()));
    };
  }, [refreshTracks]);

  const onCreated = useCallback(
    (t: Track) => {
      setTracks((prev) => [t, ...prev]);
      setTab("library");
    },
    [],
  );

  const ready = status?.state === "ready";
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
              onClick={() => setLargeText((v) => !v)}
              aria-pressed={largeText}
            >
              {largeText ? "Normal text" : "Larger text"}
            </button>
          </div>
        </header>

        <div className="tabs" role="tablist" aria-label="Sections">
          <button
            type="button"
            role="tab"
            id="tab-create"
            className="tab"
            aria-selected={tab === "create"}
            aria-controls="panel-create"
            onClick={() => setTab("create")}
          >
            Create
          </button>
          <button
            type="button"
            role="tab"
            id="tab-library"
            className="tab"
            aria-selected={tab === "library"}
            aria-controls="panel-library"
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
            <div className="notice notice-warn">
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
            <div className="notice notice-warn">
              <p>
                <strong>Some model files are missing</strong>
                Aria needs a language model, a text encoder, a sound model and a
                decoder before it can make music.
              </p>
            </div>
          )}

          {tab === "create" ? (
            <div role="tabpanel" id="panel-create" aria-labelledby="tab-create">
              <Create onCreated={onCreated} engineReady={!!ready} />
            </div>
          ) : (
            <div role="tabpanel" id="panel-library" aria-labelledby="tab-library">
              <LibraryView
                tracks={tracks}
                onChanged={refreshTracks}
                supportsStems={!!status?.supports_stems}
                busy={deriving}
                onDeriveStarted={() => setDeriving(true)}
              />
            </div>
          )}
        </main>

        <footer className="footer">
          <span>
            <span
              className={
                "status-dot " +
                (ready ? "status-ready" : status?.state === "starting" ? "status-busy" : "status-stopped")
              }
              aria-hidden="true"
            />
            {ready
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
