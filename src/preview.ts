/**
 * Browser-preview mode.
 *
 * Aria is a desktop app, but being able to open the interface in a plain browser
 * is worth the small amount of code here: design and accessibility work (focus
 * order, screen-reader output, contrast, reduced motion) can be iterated in
 * seconds instead of waiting on a full Rust rebuild each time.
 *
 * Nothing here runs in the real app — every path is behind `isDesktop`.
 */

import type { EngineStatus, Track } from "./types";

/** True when running inside Tauri; false in a normal browser tab. */
export const isDesktop =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const SAMPLE_LYRICS = `[Intro - Acoustic Guitar]

[Verse 1]
In a city full of whispers
We found a quiet place
Beneath a moon made of silver
We carved out our own space

[Chorus]
In this midnight waltz, love's secret melody
With you in my arms, we set our spirits free`;

/**
 * A stand-in cover for browser-preview mode.
 *
 * The real artwork is drawn in Rust (`src-tauri/src/art.rs`) and fetched from
 * the local audio server, which doesn't exist in a plain browser tab. This is
 * deliberately *not* a port of that generator — two implementations of the same
 * picture would drift. It only has to fill the same space so layout, focus
 * order and alt text can be checked.
 */
export function previewArt(id: string): string {
  let h = 2166136261;
  for (const ch of id) h = Math.imul(h ^ ch.charCodeAt(0), 16777619);
  const hue = Math.abs(h) % 360;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
    `<stop offset="0" stop-color="hsl(${hue} 60% 16%)"/>` +
    `<stop offset="1" stop-color="hsl(${(hue + 140) % 360} 65% 42%)"/>` +
    `</linearGradient></defs>` +
    `<rect width="640" height="640" fill="url(#g)"/></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

export const previewTracks: Track[] = [
  {
    id: "preview-1",
    title: "Warm indie folk with fingerpicked",
    prompt: "warm indie folk with fingerpicked acoustic guitar",
    caption: "A warm and intimate indie folk track built around a clean, fingerpicked guitar.",
    lyrics: SAMPLE_LYRICS,
    bpm: 158,
    keyscale: "B♭ major",
    timesignature: "4",
    vocal_language: "en",
    duration: 59.86,
    seed: 670420575,
    model: "acestep-v15-turbo-Q8_0.gguf",
    audio_path: "",
    latent_path: null,
    created_at: Math.floor(Date.now() / 1000) - 600,
    parent_id: null,
    operation: null,
    favorite: true,
  },
  {
    id: "preview-2",
    title: "Dreamy lo-fi hip hop, mellow rhodes",
    prompt: "dreamy lo-fi hip hop, mellow rhodes piano, soft vinyl crackle",
    caption: "Mellow lo-fi beat with rhodes piano and vinyl texture.",
    lyrics: "",
    bpm: 82,
    keyscale: "F minor",
    timesignature: "4",
    vocal_language: null,
    duration: 29.45,
    seed: 12345,
    model: "acestep-v15-turbo-Q8_0.gguf",
    audio_path: "",
    latent_path: null,
    created_at: Math.floor(Date.now() / 1000) - 3600,
    parent_id: null,
    operation: null,
    favorite: false,
  },
];

export const previewStatus: EngineStatus = {
  state: "ready",
  models: {
    lm: ["acestep-5Hz-lm-1.7B-Q8_0.gguf"],
    embedding: ["Qwen3-Embedding-0.6B-Q8_0.gguf"],
    vae: ["vae-BF16.gguf"],
    dit_turbo: ["acestep-v15-turbo-Q8_0.gguf"],
    dit_sft: ["acestep-v15-sft-Q8_0.gguf"],
  },
  models_complete: true,
  supports_stems: true,
  vae_chunk: 128,
  cpu_fallback: false,
};

/** Walks the real stage sequence so progress and announcements can be checked. */
export function runPreviewGeneration(
  emit: (stage: string, detail: string) => void,
  done: (t: Track) => void,
): void {
  const script: Array<[string, string, number]> = [
    ["starting", "Waking the engine", 700],
    ["writing", "Writing the song", 3000],
    ["rendering", "Recording the audio", 3500],
    ["saving", "Saving to your library", 600],
  ];
  let delay = 0;
  for (const [stage, detail, ms] of script) {
    setTimeout(() => emit(stage, detail), delay);
    delay += ms;
  }
  setTimeout(() => {
    done({
      ...previewTracks[0],
      id: `preview-${Date.now()}`,
      title: "Your new song",
      created_at: Math.floor(Date.now() / 1000),
      favorite: false,
    });
  }, delay);
}
