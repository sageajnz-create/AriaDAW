import { useEffect, useState } from "react";
import { api } from "../api";
import type { StemChoice, Track } from "../types";

/** Things you can make from a song you already have.
 *  Suno charges for stem separation and section editing; here they're just
 *  part of the app. */
type Panel = "stems" | "extend" | "cover" | "section" | null;

/** mm:ss for the section editor, where seconds matter. */
function clock(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

interface Props {
  track: Track;
  supportsStems: boolean;
  busy: boolean;
  onStarted: (jobId: string) => void;
}

export default function Derive({ track, supportsStems, busy, onStarted }: Props) {
  const [panel, setPanel] = useState<Panel>(null);
  const [stems, setStems] = useState<StemChoice[]>([]);
  const [stem, setStem] = useState("vocals");
  const [extendBy, setExtendBy] = useState(30);
  const [coverStyle, setCoverStyle] = useState("");
  const [secStart, setSecStart] = useState(0);
  const [secEnd, setSecEnd] = useState(Math.min(15, Math.floor(track.duration)));
  const [secStyle, setSecStyle] = useState("");

  useEffect(() => {
    if (panel === "stems" && stems.length === 0) {
      api.stemChoices().then(setStems).catch(() => {});
    }
  }, [panel, stems.length]);

  async function run(operation: Record<string, unknown>) {
    try {
      onStarted(await api.deriveTrack(track.id, operation));
      setPanel(null);
    } catch (e) {
      console.error(e);
    }
  }

  return (
    <div className="derive">
      <div className="btn-row">
        <button
          type="button"
          className="btn btn-icon"
          onClick={() => setPanel(panel === "stems" ? null : "stems")}
          aria-expanded={panel === "stems"}
          disabled={busy || !supportsStems}
          title={
            supportsStems
              ? "Pull one instrument or voice out on its own"
              : "Needs the detailed sound model"
          }
        >
          Separate parts
        </button>
        <button
          type="button"
          className="btn btn-icon"
          onClick={() => setPanel(panel === "extend" ? null : "extend")}
          aria-expanded={panel === "extend"}
          disabled={busy}
        >
          Make longer
        </button>
        <button
          type="button"
          className="btn btn-icon"
          onClick={() => setPanel(panel === "cover" ? null : "cover")}
          aria-expanded={panel === "cover"}
          disabled={busy}
        >
          Change the style
        </button>
        <button
          type="button"
          className="btn btn-icon"
          onClick={() => setPanel(panel === "section" ? null : "section")}
          aria-expanded={panel === "section"}
          disabled={busy}
          title="Regenerate just one part, keeping the rest"
        >
          Redo a section
        </button>
      </div>

      {panel === "section" && (
        <div className="derive-panel">
          <p className="hint" style={{ marginTop: 0 }}>
            Keeps the whole song and rewrites only the part you choose — handy when
            one verse doesn't land but the rest does.
          </p>
          <div className="grid-2">
            <div className="field">
              <label htmlFor={`s0-${track.id}`}>Start at {clock(secStart)}</label>
              <input
                id={`s0-${track.id}`}
                type="range"
                min={0}
                max={Math.max(1, Math.floor(track.duration) - 1)}
                step={1}
                value={secStart}
                onChange={(e) => {
                  const v = Number(e.target.value);
                  setSecStart(v);
                  if (v >= secEnd) setSecEnd(Math.min(Math.floor(track.duration), v + 5));
                }}
                disabled={busy}
                aria-valuetext={clock(secStart)}
              />
            </div>
            <div className="field">
              <label htmlFor={`s1-${track.id}`}>End at {clock(secEnd)}</label>
              <input
                id={`s1-${track.id}`}
                type="range"
                min={1}
                max={Math.max(2, Math.floor(track.duration))}
                step={1}
                value={secEnd}
                onChange={(e) => setSecEnd(Math.max(secStart + 1, Number(e.target.value)))}
                disabled={busy}
                aria-valuetext={clock(secEnd)}
              />
            </div>
          </div>
          <div className="field">
            <label htmlFor={`ss-${track.id}`}>
              How should that part sound? (optional)
            </label>
            <input
              id={`ss-${track.id}`}
              type="text"
              value={secStyle}
              onChange={(e) => setSecStyle(e.target.value)}
              placeholder="quieter, just piano and voice"
              disabled={busy}
            />
          </div>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() =>
              run({
                kind: "repaint",
                start: secStart,
                end: secEnd,
                caption: secStyle.trim() || null,
              })
            }
            disabled={busy || secEnd <= secStart}
          >
            Redo {clock(secStart)}–{clock(secEnd)}
          </button>
        </div>
      )}

      {panel === "stems" && (
        <div className="derive-panel">
          <div className="field">
            <label htmlFor={`stem-${track.id}`}>Which part do you want on its own?</label>
            <select
              id={`stem-${track.id}`}
              value={stem}
              onChange={(e) => setStem(e.target.value)}
              disabled={busy}
            >
              {stems.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
            <p className="hint">
              Makes a new track with just that part. The original stays as it is.
            </p>
          </div>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => run({ kind: "stem", track: stem })}
            disabled={busy}
          >
            Separate it
          </button>
        </div>
      )}

      {panel === "extend" && (
        <div className="derive-panel">
          <div className="field">
            <label htmlFor={`ext-${track.id}`}>
              How much longer? {extendBy} seconds
            </label>
            <input
              id={`ext-${track.id}`}
              type="range"
              min={10}
              max={120}
              step={10}
              value={extendBy}
              onChange={(e) => setExtendBy(Number(e.target.value))}
              disabled={busy}
              aria-valuetext={`${extendBy} seconds`}
            />
            <p className="hint">Aria continues the song past where it currently ends.</p>
          </div>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => run({ kind: "extend", seconds: extendBy })}
            disabled={busy}
          >
            Make it longer
          </button>
        </div>
      )}

      {panel === "cover" && (
        <div className="derive-panel">
          <div className="field">
            <label htmlFor={`cov-${track.id}`}>What should it sound like instead?</label>
            <input
              id={`cov-${track.id}`}
              type="text"
              value={coverStyle}
              onChange={(e) => setCoverStyle(e.target.value)}
              placeholder="slow piano ballad with strings"
              disabled={busy}
            />
            <p className="hint">
              Keeps the shape of the song but performs it in a different style.
            </p>
          </div>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() =>
              run({ kind: "cover", caption: coverStyle.trim(), strength: 0.6 })
            }
            disabled={busy || !coverStyle.trim()}
          >
            Change the style
          </button>
        </div>
      )}
    </div>
  );
}
