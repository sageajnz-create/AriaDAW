import { useEffect, useState } from "react";
import { api, onEvent } from "../api";
import type { SetupInfo, SetupProgress, Tier } from "../types";

/** Human sizes. Nobody thinks in bytes. */
function gb(bytes: number): string {
  const g = bytes / 1e9;
  return g >= 1 ? `${g.toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
}

const TIERS: Tier[] = ["light", "standard", "best"];

interface Props {
  onReady: () => void;
}

/**
 * First run: fetch the models.
 *
 * They're about 12 GB, so they can't ship inside the app and every install has
 * to download them once. This is the first thing anyone sees, so it explains
 * what's happening and why, and never shows a filename where a plain word will
 * do.
 */
export default function Setup({ onReady }: Props) {
  const [info, setInfo] = useState<SetupInfo | null>(null);
  const [tier, setTier] = useState<Tier | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<SetupProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.setupInfo(tier ?? undefined).then((i) => {
      setInfo(i);
      if (!tier) setTier(i.tier);
    }).catch((e) => setError(String(e)));
  }, [tier]);

  useEffect(() => {
    const offs = [
      onEvent<SetupProgress>("setup:progress", setProgress),
      onEvent<Record<string, never>>("setup:done", () => {
        setBusy(false);
        setProgress(null);
        onReady();
      }),
      onEvent<{ message: string }>("setup:error", (p) => {
        setBusy(false);
        setError(p.message);
      }),
    ];
    return () => {
      offs.forEach((o) => o.then((f) => f()));
    };
  }, [onReady]);

  async function start() {
    if (!tier) return;
    setError(null);
    setBusy(true);
    try {
      await api.downloadModels(tier);
    } catch (e) {
      setBusy(false);
      setError(String(e));
    }
  }

  if (!info) {
    return (
      <div className="setup">
        <p>Checking what's on your computer…</p>
      </div>
    );
  }

  const pct =
    progress && progress.total > 0
      ? Math.round((progress.downloaded / progress.total) * 100)
      : 0;

  return (
    <div className="setup">
      <h2>One-time setup</h2>
      <p className="setup-lead">
        Aria makes music on your own computer, so it needs to download the models
        that do the work. This happens once. After that, everything works offline
        and nothing you make ever leaves your machine.
      </p>

      {!busy && (
        <>
          <div className="field">
            <label htmlFor="tier">How much should Aria download?</label>
            <div className="tier-list" role="radiogroup" aria-label="Download size">
              {TIERS.map((t) => {
                const selected = tier === t;
                const isSuggested = info.tier === t;
                return (
                  <button
                    key={t}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    className={`tier${selected ? " tier-on" : ""}`}
                    onClick={() => setTier(t)}
                  >
                    <span className="tier-name">
                      {t === "light" ? "Light" : t === "standard" ? "Standard" : "Best"}
                      {isSuggested && <span className="tier-tag">Suits your computer</span>}
                    </span>
                    <span className="tier-desc">
                      {selected ? info.tier_description : ""}
                    </span>
                  </button>
                );
              })}
            </div>
            <p className="hint">
              {info.vram_mb
                ? `Your graphics card has about ${Math.round(info.vram_mb / 1024)} GB of memory.`
                : "Aria couldn't detect a graphics card, so it suggested a safe option."}
              {" "}You can change this later.
            </p>
          </div>

          <div className="notice notice-info">
            <p>
              <strong>{gb(info.missing_bytes)} to download</strong>
              {info.missing_count === 0
                ? "Everything is already here."
                : `${info.missing_count} file${info.missing_count === 1 ? "" : "s"}. You can stop and come back — it picks up where it left off.`}
            </p>
          </div>

          <div className="btn-row">
            <button
              type="button"
              className="btn btn-primary"
              onClick={info.ready ? onReady : start}
            >
              {info.ready ? "Start making music" : "Download and set up"}
            </button>
          </div>
        </>
      )}

      {/* Announced politely so progress is audible, not just visible. */}
      <div aria-live="polite" aria-atomic="true">
        {busy && (
          <div className="progress">
            <div className="progress-head">
              <span className="pulse" aria-hidden="true" />
              <span>
                {progress
                  ? `Getting the ${progress.role} (${progress.file_index + 1} of ${progress.file_count})`
                  : "Starting the download"}
              </span>
            </div>
            {progress && (
              <>
                <div
                  className="bar"
                  role="progressbar"
                  aria-valuenow={pct}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-label="Download progress"
                >
                  <div className="bar-fill" style={{ width: `${pct}%` }} />
                </div>
                <p className="progress-detail">
                  {gb(progress.downloaded)} of {gb(progress.total)} — {pct}%
                </p>
              </>
            )}
            <p className="progress-elapsed">
              You can leave this running. If it stops, reopening Aria continues
              from where it got to.
            </p>
          </div>
        )}
      </div>

      {error && (
        <div className="notice notice-err" role="alert">
          <p>
            <strong>The download didn't finish</strong>
            {error} Anything already downloaded is kept, so trying again resumes.
          </p>
          <div className="btn-row" style={{ marginTop: 12 }}>
            <button type="button" className="btn" onClick={start}>
              Try again
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
