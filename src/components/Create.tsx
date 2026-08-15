import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { StageEvent, Track } from "../types";

/** Plain-language labels. No jargon — the person using this may not make music. */
const STAGE_TEXT: Record<string, string> = {
  starting: "Warming up",
  writing: "Writing the words and melody",
  rendering: "Recording the audio",
  recovering: "Adjusting for your computer",
  saving: "Saving to your library",
};

interface Props {
  onCreated: (t: Track) => void;
  engineReady: boolean;
}

export default function Create({ onCreated, engineReady }: Props) {
  const [prompt, setPrompt] = useState("");
  const [instrumental, setInstrumental] = useState(false);
  const [duration, setDuration] = useState(60);
  const [busy, setBusy] = useState(false);
  const [stage, setStage] = useState<string | null>(null);
  const [detail, setDetail] = useState("");
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const jobRef = useRef<string | null>(null);

  // Elapsed timer — local generation takes as long as it takes, and showing
  // real time is more honest than a fake progress bar.
  useEffect(() => {
    if (!busy) return;
    const t = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(t);
  }, [busy]);

  useEffect(() => {
    const unlisten: Array<Promise<() => void>> = [];

    unlisten.push(
      listen<StageEvent>("gen:stage", (e) => {
        if (jobRef.current && e.payload.job_id !== jobRef.current) return;
        setStage(e.payload.stage);
        setDetail(e.payload.detail);
      }),
    );

    unlisten.push(
      listen<{ job_id: string; track: Track }>("gen:done", (e) => {
        if (jobRef.current && e.payload.job_id !== jobRef.current) return;
        setBusy(false);
        setStage(null);
        jobRef.current = null;
        onCreated(e.payload.track);
      }),
    );

    unlisten.push(
      listen<{ job_id: string; message: string }>("gen:error", (e) => {
        if (jobRef.current && e.payload.job_id !== jobRef.current) return;
        setBusy(false);
        setStage(null);
        jobRef.current = null;
        setError(e.payload.message);
      }),
    );

    return () => {
      unlisten.forEach((p) => p.then((f) => f()));
    };
  }, [onCreated]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!prompt.trim() || busy) return;
    setError(null);
    setBusy(true);
    setElapsed(0);
    setStage("starting");
    setDetail("");
    try {
      jobRef.current = await api.generate({
        prompt: prompt.trim(),
        instrumental,
        duration,
      });
    } catch (err) {
      setBusy(false);
      setStage(null);
      setError(String(err));
    }
  }

  return (
    <div>
      <p className="create-intro">
        Describe the song you want, in your own words. Aria writes the lyrics, picks
        the key and tempo, and records it on your computer.
      </p>

      <form onSubmit={submit}>
        <div className="field">
          <label htmlFor="prompt">What should the song be like?</label>
          <textarea
            id="prompt"
            className="prompt-box"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="A gentle piano song about missing someone in the autumn"
            aria-describedby="prompt-hint"
            disabled={busy}
            required
          />
          <p className="hint" id="prompt-hint">
            Mention a feeling, a story, instruments, or a style — whatever matters to
            you. There's no wrong way to write this.
          </p>
        </div>

        <div className="field">
          <label htmlFor="duration">How long? {formatMinutes(duration)}</label>
          <input
            id="duration"
            type="range"
            min={20}
            max={240}
            step={10}
            value={duration}
            onChange={(e) => setDuration(Number(e.target.value))}
            disabled={busy}
            aria-valuetext={formatMinutes(duration)}
          />
        </div>

        <div className="field">
          <label className="check" htmlFor="instrumental">
            <input
              id="instrumental"
              type="checkbox"
              checked={instrumental}
              onChange={(e) => setInstrumental(e.target.checked)}
              disabled={busy}
            />
            Instrumental — no singing
          </label>
        </div>

        <div className="btn-row">
          <button
            type="submit"
            className="btn btn-primary"
            disabled={busy || !prompt.trim() || !engineReady}
          >
            {busy ? "Making your song…" : "Make my song"}
          </button>
          {!engineReady && !busy && (
            <span className="hint">Starting the engine…</span>
          )}
        </div>
      </form>

      {/* Progress is announced politely so screen-reader users hear each step. */}
      <div aria-live="polite" aria-atomic="true">
        {busy && stage && (
          <div className="progress">
            <div className="progress-head">
              <span className="pulse" aria-hidden="true" />
              <span>{STAGE_TEXT[stage] ?? "Working"}</span>
            </div>
            {detail && <p className="progress-detail">{detail}</p>}
            <p className="progress-elapsed">{elapsed}s elapsed</p>
          </div>
        )}
      </div>

      {error && (
        <div className="notice notice-err" role="alert">
          <p>
            <strong>That didn't work</strong>
            {error}
          </p>
        </div>
      )}
    </div>
  );
}

function formatMinutes(seconds: number): string {
  if (seconds < 60) return `${seconds} seconds`;
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  if (s === 0) return m === 1 ? "1 minute" : `${m} minutes`;
  return `${m} min ${s} sec`;
}
