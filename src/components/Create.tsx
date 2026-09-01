import { useEffect, useRef, useState } from "react";
import {
  STAGE_LABELS,
  generationFinished,
  generationProgress,
  generationStarted,
  useAnnounce,
} from "../a11y";
import { api, onEvent } from "../api";
import { isDesktop, runPreviewGeneration } from "../preview";
import Studio, { studioToOptions, type StudioValues } from "./Studio";
import type { Persona, StageEvent, Track } from "../types";

interface Props {
  onCreated: (t: Track) => void;
  engineReady: boolean;
  /** Existing tracks, offered as voice references. */
  tracks: Track[];
  /** Voices the user has saved and named. */
  personas: Persona[];
  onPersonasChanged: () => void;
  /** Instruct model writing the lyrics, or null when none is reachable. */
  lyricWriter: string | null;
}

export default function Create({
  onCreated, engineReady, tracks, personas, onPersonasChanged, lyricWriter,
}: Props) {
  const [prompt, setPrompt] = useState("");
  const [instrumental, setInstrumental] = useState(false);
  const [duration, setDuration] = useState(60);
  const [language, setLanguage] = useState("en");
  const [languages, setLanguages] = useState<Array<[string, string]>>([]);
  const [busy, setBusy] = useState(false);
  const [stage, setStage] = useState<string | null>(null);
  const [detail, setDetail] = useState("");
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [studioOpen, setStudioOpen] = useState(false);
  const [studio, setStudio] = useState<StudioValues>({
    lyrics: "",
    bpm: 0,
    keyscale: "",
    seed: "",
    // On by default: turning it off produces skeletal lyrics and often no
    // vocals at all. See GenerateOptions::embellish in the Rust side.
    embellish: true,
    quality: "best",
    variations: 1,
    format: "mp3",
    voiceFrom: "",
  });
  const jobRef = useRef<string | null>(null);
  const lyricsRef = useRef<HTMLTextAreaElement>(null);
  const announce = useAnnounce();

  // Seed the language picker from the user's locale. Without an explicit
  // language the model invents one, which is how an English reggae prompt came
  // back sung in Hindi.
  useEffect(() => {
    (async () => {
      const [list, dflt] = await Promise.all([api.languages(), api.defaultLanguage()]);
      setLanguages(list);
      if (list.some(([code]) => code === dflt)) setLanguage(dflt);
    })();
  }, []);

  // Elapsed timer. Local generation takes as long as it takes, and showing real
  // seconds is more honest than animating a progress bar we can't predict.
  useEffect(() => {
    if (!busy) return;
    const t = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(t);
  }, [busy]);

  useEffect(() => {
    const offs: Array<Promise<() => void>> = [];

    offs.push(
      onEvent<StageEvent>("gen:stage", (p) => {
        if (jobRef.current && p.job_id !== jobRef.current) return;
        setStage(p.stage);
        setDetail(p.detail);
      }),
    );

    offs.push(
      onEvent<{ job_id: string; track: Track }>("gen:track", (p) => {
        if (jobRef.current && p.job_id !== jobRef.current) return;
        onCreated(p.track);
      }),
    );

    offs.push(
      onEvent<{ job_id: string; track: Track }>("gen:done", (p) => {
        if (jobRef.current && p.job_id !== jobRef.current) return;
        setBusy(false);
        setStage(null);
        jobRef.current = null;
      }),
    );

    offs.push(
      onEvent<{ job_id: string; message: string }>("gen:error", (p) => {
        if (jobRef.current && p.job_id !== jobRef.current) return;
        setBusy(false);
        setStage(null);
        jobRef.current = null;
        setError(p.message);
      }),
    );

    return () => {
      offs.forEach((p) => p.then((f) => f()));
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
    announce(generationStarted());

    if (!isDesktop) {
      runPreviewGeneration(
        (s, d) => {
          setStage(s);
          setDetail(d);
          announce(generationProgress(s, d));
        },
        (t) => {
          setBusy(false);
          setStage(null);
          announce(generationFinished(t.title));
          onCreated(t);
        },
      );
      return;
    }

    try {
      jobRef.current = await api.generate({
        prompt: prompt.trim(),
        instrumental,
        duration,
        vocal_language: language,
        // Always applied. Gating these on the panel being open meant anyone who
        // set something and then collapsed the panel lost their choice without
        // being told. Every value has a sensible default, so there is nothing
        // to protect a Simple-mode song from.
        ...studioToOptions(studio),
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

      <form onSubmit={submit} aria-busy={busy}>
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

        {!instrumental && !lyricWriter && (
          <div className="notice notice-warn" role="status">
            <strong>The words may not be about what you asked for.</strong>
            <p>
              Aria writes lyrics with a small language model running on this
              computer, and it can't find one. Without it the music model makes
              the words up from its own description of the sound — they usually
              aren't about your subject, and sometimes come out in another
              language. The music itself is unaffected.
            </p>
            <p>
              Install <strong>Ollama</strong> and run{" "}
              <code>ollama pull llama3.2:3b</code> (about 2 GB, stays on this
              computer), then start a new song.
            </p>
          </div>
        )}

        {!instrumental && (
          <div className="field">
            <label htmlFor="language">What language should it be sung in?</label>
            <select
              id="language"
              value={language}
              onChange={(e) => setLanguage(e.target.value)}
              disabled={busy}
              aria-describedby="language-hint"
            >
              {languages.map(([code, name]) => (
                <option key={code} value={code}>
                  {name}
                </option>
              ))}
            </select>
            <p className="hint" id="language-hint">
              Aria can sing in over 50 languages. This starts on your computer's
              language — change it any time.
            </p>
          </div>
        )}

        <div className="field">
          <label className="check" htmlFor="instrumental">
            <input
              id="instrumental"
              type="checkbox"
              checked={instrumental}
              onChange={(e) => setInstrumental(e.target.checked)}
              disabled={busy}
            />
            <span>Instrumental — no singing</span>
          </label>
        </div>

        <div className="field">
          <button
            type="button"
            className="btn disclosure"
            onClick={() => setStudioOpen((v) => !v)}
            aria-expanded={studioOpen}
            aria-controls="studio-panel"
            disabled={busy}
          >
            <span aria-hidden="true">{studioOpen ? "▾" : "▸"}</span>
            {studioOpen ? "Hide the detailed controls" : "Write my own words, or set the tempo and key"}
          </button>
        </div>

        {studioOpen && (
          <div id="studio-panel">
            <Studio
              values={studio}
              onChange={(v) => setStudio((s) => ({ ...s, ...v }))}
              disabled={busy}
              lyricsRef={lyricsRef}
              voiceChoices={tracks
                .filter((t) => !t.missing)
                .slice(0, 40)
                .map((t) => ({ id: t.id, title: t.title }))}
              personas={personas}
              onPersonasChanged={onPersonasChanged}
            />
          </div>
        )}

        <div className="btn-row">
          <button
            type="submit"
            className="btn btn-primary"
            disabled={busy || !prompt.trim() || !engineReady}
          >
            {busy ? "Making your song…" : "Make my song"}
          </button>
          {!engineReady && !busy && <span className="hint">Starting the engine…</span>}
        </div>
      </form>

      {/* Visual progress only. The window-level live region speaks start, each
          stage, and completion — including after this tab unmounts. Elapsed
          seconds stay here so they are not re-announced every tick. */}
      {busy && stage && (
        <div className="progress">
          <div className="progress-head">
            <span className="pulse" aria-hidden="true" />
            <span>{STAGE_LABELS[stage] ?? "Working"}</span>
          </div>
          {detail && <p className="progress-detail">{detail}</p>}
          <p className="progress-elapsed">{elapsed}s elapsed</p>
        </div>
      )}

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
