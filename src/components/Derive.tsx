import { useEffect, useState } from "react";
import { api } from "../api";
import type { StemChoice, Track } from "../types";

/** Things you can make from a song you already have.
 *  Suno charges for stem separation and section editing; here they're just
 *  part of the app. */
type Panel = "stems" | "extend" | "cover" | null;

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
      </div>

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
