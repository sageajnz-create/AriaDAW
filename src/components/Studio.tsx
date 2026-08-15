import type { GenerateOptions } from "../types";

/** Structure tags the model understands. Inserted for the user so nobody has to
 *  memorise syntax to get a chorus. */
const TAGS = [
  "[Intro]", "[Verse]", "[Pre-Chorus]", "[Chorus]", "[Bridge]",
  "[Guitar Solo]", "[Instrumental Break]", "[Outro]",
];

const KEYS = [
  "", "C major", "G major", "D major", "A major", "E major", "F major",
  "B♭ major", "E♭ major", "A minor", "E minor", "B minor", "D minor",
  "G minor", "C minor", "F# minor",
];

export interface StudioValues {
  lyrics: string;
  bpm: number;
  keyscale: string;
  seed: string;
  embellish: boolean;
  quality: "best" | "fast";
  variations: number;
  format: "mp3" | "wav24";
  voiceFrom: string;
}

interface Props {
  values: StudioValues;
  onChange: (v: Partial<StudioValues>) => void;
  disabled: boolean;
  lyricsRef: React.RefObject<HTMLTextAreaElement>;
  /** Tracks that can act as a voice reference. */
  voiceChoices: Array<{ id: string; title: string }>;
}

export default function Studio({
  values, onChange, disabled, lyricsRef, voiceChoices,
}: Props) {
  /** Insert a tag at the cursor, keeping focus where the user was typing. */
  function insertTag(tag: string) {
    const el = lyricsRef.current;
    if (!el) {
      onChange({ lyrics: `${values.lyrics}\n${tag}\n` });
      return;
    }
    const start = el.selectionStart ?? values.lyrics.length;
    const end = el.selectionEnd ?? start;
    const before = values.lyrics.slice(0, start);
    const after = values.lyrics.slice(end);
    const insert = `${before && !before.endsWith("\n") ? "\n" : ""}${tag}\n`;
    const next = before + insert + after;
    onChange({ lyrics: next });
    // Restore the caret after React re-renders.
    requestAnimationFrame(() => {
      el.focus();
      const pos = before.length + insert.length;
      el.setSelectionRange(pos, pos);
    });
  }

  return (
    <div className="studio">
      <div className="field">
        <label htmlFor="lyrics">Your words</label>
        <textarea
          id="lyrics"
          className="lyrics-box"
          value={values.lyrics}
          onChange={(e) => onChange({ lyrics: e.target.value })}
          placeholder={"[Verse]\nWrite your own words here…\n\n[Chorus]\nOr leave this empty and Aria will write them for you"}
          disabled={disabled}
          aria-describedby="lyrics-hint"
          ref={lyricsRef}
          rows={10}
        />
        <p className="hint" id="lyrics-hint">
          Leave this empty and Aria writes the lyrics itself. Use the buttons below
          to mark sections — they tell the music where to change.
        </p>
        <div className="tag-row" role="group" aria-label="Insert a section marker">
          {TAGS.map((t) => (
            <button
              key={t}
              type="button"
              className="btn btn-tag"
              onClick={() => insertTag(t)}
              disabled={disabled}
            >
              {t}
            </button>
          ))}
        </div>
      </div>

      <div className="grid-2">
        <div className="field">
          <label htmlFor="bpm">
            Tempo {values.bpm > 0 ? `— ${values.bpm} BPM` : "— Aria decides"}
          </label>
          <input
            id="bpm"
            type="range"
            min={0}
            max={200}
            step={1}
            value={values.bpm}
            onChange={(e) => onChange({ bpm: Number(e.target.value) })}
            disabled={disabled}
            aria-valuetext={values.bpm > 0 ? `${values.bpm} beats per minute` : "Aria decides"}
          />
          <p className="hint">Slide to the far left to let Aria choose.</p>
        </div>

        <div className="field">
          <label htmlFor="keyscale">Key</label>
          <select
            id="keyscale"
            value={values.keyscale}
            onChange={(e) => onChange({ keyscale: e.target.value })}
            disabled={disabled}
          >
            {KEYS.map((k) => (
              <option key={k || "auto"} value={k}>
                {k || "Aria decides"}
              </option>
            ))}
          </select>
          <p className="hint">Major tends to sound bright; minor tends to sound sad.</p>
        </div>
      </div>

      <div className="field">
        <label htmlFor="seed">Seed</label>
        <input
          id="seed"
          type="text"
          inputMode="numeric"
          value={values.seed}
          onChange={(e) => onChange({ seed: e.target.value.replace(/[^0-9]/g, "") })}
          placeholder="Leave empty for a new song each time"
          disabled={disabled}
          aria-describedby="seed-hint"
        />
        <p className="hint" id="seed-hint">
          The same seed with the same settings makes the same song again. Every song
          you make records its seed, so you can always get back to it.
        </p>
      </div>

      <div className="grid-2">
        <div className="field">
          <label htmlFor="variations">
            How many versions? {values.variations === 1 ? "Just one" : values.variations}
          </label>
          <input
            id="variations"
            type="range"
            min={1}
            max={4}
            step={1}
            value={values.variations}
            onChange={(e) => onChange({ variations: Number(e.target.value) })}
            disabled={disabled}
            aria-valuetext={`${values.variations} version${values.variations > 1 ? "s" : ""}`}
          />
          <p className="hint">
            Same words, different performances, so you can pick a favourite. Each
            one adds to the wait.
          </p>
        </div>

        <div className="field">
          <label htmlFor="format">Save as</label>
          <select
            id="format"
            value={values.format}
            onChange={(e) => onChange({ format: e.target.value as "mp3" | "wav24" })}
            disabled={disabled}
          >
            <option value="mp3">MP3 — 320 kbps, small files</option>
            <option value="wav24">WAV — lossless, about 10x bigger</option>
          </select>
          <p className="hint">Choose WAV if you're taking this into a music app.</p>
        </div>
      </div>

      {voiceChoices.length > 0 && (
        <div className="field">
          <label htmlFor="voiceFrom">Sing it in the voice from…</label>
          <select
            id="voiceFrom"
            value={values.voiceFrom}
            onChange={(e) => onChange({ voiceFrom: e.target.value })}
            disabled={disabled}
            aria-describedby="voice-hint"
          >
            <option value="">A new voice each time</option>
            {voiceChoices.map((t) => (
              <option key={t.id} value={t.id}>
                {t.title}
              </option>
            ))}
          </select>
          <p className="hint" id="voice-hint">
            Borrows the singer's tone from a song you already have, without
            copying its tune or words — so one voice can carry across everything
            you make. Works with imported recordings too.
          </p>
        </div>
      )}

      <div className="field">
        <label htmlFor="quality">Sound quality</label>
        <select
          id="quality"
          value={values.quality}
          onChange={(e) =>
            onChange({ quality: e.target.value as "best" | "fast" })
          }
          disabled={disabled}
          aria-describedby="quality-hint"
        >
          <option value="best">Best — better words and richer sound (about a minute)</option>
          <option value="fast">Fast — rougher, good for trying ideas (about 25 seconds)</option>
        </select>
        <p className="hint" id="quality-hint">
          Best uses larger models that write more coherent lyrics, stay in the
          language you picked, and produce fuller sound. Both are free and
          unlimited — the only difference is how long you wait.
        </p>
      </div>

      <div className="field">
        <label className="check" htmlFor="embellish">
          <input
            id="embellish"
            type="checkbox"
            checked={values.embellish}
            onChange={(e) => onChange({ embellish: e.target.checked })}
            disabled={disabled}
          />
          <span>Let Aria flesh out my description (recommended)</span>
        </label>
        <p className="hint">
          Aria adds its own musical detail, which is what makes songs sound full
          and produces proper sung lyrics. Turning this off keeps your words
          exactly as written, but songs come out thinner and often have no
          singing at all.
        </p>
      </div>
    </div>
  );
}

/** Fold studio values into the request the backend expects. */
export function studioToOptions(v: StudioValues): Partial<GenerateOptions> {
  return {
    lyrics: v.lyrics.trim(),
    bpm: v.bpm > 0 ? v.bpm : null,
    keyscale: v.keyscale || null,
    seed: v.seed ? Number(v.seed) : null,
    embellish: v.embellish,
    quality: v.quality,
    variations: v.variations,
    format: v.format,
    voice_from: v.voiceFrom || null,
  };
}
