/**
 * Plain-language strings for the polite live region.
 *
 * Generation progress used to live inside the visible progress card, which
 * also counted elapsed seconds. Because that card was `aria-atomic`, a screen
 * reader heard the whole block again every second for the entire wait — and
 * never heard "done", because the card unmounted when the job finished (and
 * the Create tab unmounts the moment the first take lands in My songs).
 *
 * These helpers are the contract: start, stage changes, and completion. They
 * never include a ticking clock.
 */

/** Shown in the UI and announced. No engine jargon, no filenames. */
export const STAGE_LABELS: Record<string, string> = {
  starting: "Warming up",
  composing: "Composing the music",
  writing: "Writing the words and melody",
  rendering: "Recording the audio",
  recovering: "Adjusting for your computer",
  saving: "Saving to your library",
};

export function generationStarted(): string {
  return "Making your song.";
}

export function generationProgress(stage: string, detail?: string): string {
  const label = STAGE_LABELS[stage] ?? "Working";
  const extra = detail?.trim();
  return extra ? `${label}. ${extra}` : `${label}.`;
}

export function generationFinished(title?: string): string {
  return title
    ? `${title} is ready. It's in My songs.`
    : "Your song is ready. It's in My songs.";
}

export function deriveFinished(): string {
  return "The new version is ready. It's in My songs.";
}

/** Announce a setup file change — never the byte counter, which updates constantly. */
export function setupProgressMessage(role: string, fileIndex: number, fileCount: number): string {
  return `Getting the ${role}, file ${fileIndex + 1} of ${fileCount}.`;
}

export function setupProgressKey(role: string, fileIndex: number): string {
  return `${role}:${fileIndex}`;
}

export function setupFinished(): string {
  return "Download finished. Aria is ready.";
}

/** Same text twice in a row is not a new announcement. */
export function coalesceAnnouncement(previous: string | null, next: string): string | null {
  const trimmed = next.trim();
  if (!trimmed || trimmed === previous) return null;
  return trimmed;
}

/** BPM is producer jargon; say what it is. */
export function formatTempo(bpm: number | null | undefined): string | null {
  if (!bpm) return null;
  return `${bpm} beats per minute`;
}
