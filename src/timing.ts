/**
 * Estimated timing for follow-along lyrics.
 *
 * The engine hands back audio and words but no alignment data — it cannot say
 * when each line is sung. Forced alignment would mean shipping a speech
 * recognizer for a feature most people will simply glance at, so instead the
 * timeline is estimated from the structure the lyrics already have:
 *
 * - Section markers ([Verse 1], [Chorus]) are headings. They cost no time;
 *   they label the lines that follow.
 * - Instrumental sections ([Intro], [Guitar Solo], [Outro]) consume time
 *   without words, so they get a slice of the song sized against how long an
 *   average sung line took.
 * - Sung lines split what remains, weighted by word count — a long line takes
 *   longer to sing than a short one, on average and in expectation.
 *
 * Every number here is an estimate, and the UI says so. What keeps it honest
 * is that it never claims more precision than word-count weighting can offer:
 * lines land in order, sections stay plausible, and nothing pretends to be
 * measured.
 */

export type EntryKind = "line" | "instrumental";

export interface TimedEntry {
  kind: EntryKind;
  /** The section marker this entry sits under, if any. */
  section: string | null;
  text: string;
  start: number;
  end: number;
}

export interface LyricTimeline {
  entries: TimedEntry[];
}

const MARKER = /^\s*[([]([^)\]]*)[)\]]\s*$/;

/** Markers that mean music plays here rather than words. */
const INSTRUMENTAL_SHORT = /\b(intro|outro)\b/i;
/** A solo or break holds the song for longer than a bare intro does. */
const INSTRUMENTAL_LONG = /\b(instrumental|solo|break|interlude)\b/i;

function isMarker(text: string): string | null {
  const m = MARKER.exec(text);
  return m ? m[1].trim() : null;
}

interface Weighted {
  kind: EntryKind;
  section: string | null;
  text: string;
  weight: number;
}

/**
 * Turn lyric text into a time-weighted list. Sung lines weigh by word count;
 * instrumental sections weigh relative to the average sung line (2× for an
 * intro or outro, 4× for a solo or break). With no sung lines at all the
 * instrumentals fall back to those same fixed weights so they still divide
 * the song sensibly.
 */
export function weightLyrics(lyrics: string): Weighted[] {
  const out: Weighted[] = [];
  let section: string | null = null;

  for (const raw of lyrics.split(/\r?\n/)) {
    const text = raw.trim();
    if (!text) continue;
    const marker = isMarker(text);
    if (marker !== null) {
      if (INSTRUMENTAL_SHORT.test(marker) || INSTRUMENTAL_LONG.test(marker)) {
        out.push({ kind: "instrumental", section, text: marker, weight: 0 });
      } else {
        section = marker;
      }
      continue;
    }
    out.push({
      kind: "line",
      section,
      text,
      weight: Math.max(1, text.split(/\s+/).filter(Boolean).length),
    });
  }

  // Second pass: give instrumentals their share of an average sung line.
  const sung = out.filter((e) => e.kind === "line");
  const avg = sung.length
    ? sung.reduce((sum, e) => sum + e.weight, 0) / sung.length
    : 3; /* words — a plausible line when none exist */
  for (const e of out) {
    if (e.kind === "instrumental") {
      e.weight = avg * (INSTRUMENTAL_LONG.test(e.text) ? 4 : 2);
    }
  }
  return out;
}

/** Spread the weighted lines across `duration` seconds, in order. */
export function estimateTiming(lyrics: string, duration: number): LyricTimeline {
  const weighted = weightLyrics(lyrics);
  const total = weighted.reduce((sum, e) => sum + e.weight, 0);
  const seconds = Math.max(1, duration);
  const scale = total > 0 ? seconds / total : 0;

  let t = 0;
  const entries = weighted.map((e) => {
    const start = t;
    t += e.weight * scale;
    return { kind: e.kind, section: e.section, text: e.text, start, end: t };
  });
  return { entries };
}

/**
 * Which entry is playing at `position`: -1 before the first, otherwise the
 * index whose span contains it, clamped so the tail of the song keeps the
 * last line lit rather than dropping the highlight.
 */
export function activeIndexAt(timeline: LyricTimeline, position: number): number {
  const entries = timeline.entries;
  if (entries.length === 0 || position < entries[0].start) return -1;
  let found = entries.length - 1;
  for (let i = 0; i < entries.length; i++) {
    if (position < entries[i].end) {
      found = i;
      break;
    }
  }
  return found;
}
