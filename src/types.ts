export interface Track {
  id: string;
  title: string;
  prompt: string;
  caption: string;
  lyrics: string;
  bpm: number | null;
  keyscale: string | null;
  timesignature: string | null;
  vocal_language: string | null;
  duration: number;
  seed: number | null;
  model: string;
  audio_path: string;
  latent_path: string | null;
  created_at: number;
  parent_id: string | null;
  operation: string | null;
  favorite: boolean;
  /** Which model wrote the words, if any. */
  lyricist?: string | null;
  /** The audio file is no longer where Aria left it. */
  missing?: boolean;
}

/** Whether this computer can make a video, and why not when it can't. */
export interface VideoSupport {
  available: boolean;
  reason: string | null;
}

/** What an export actually did. */
export interface ExportReport {
  folder: string;
  written: number;
  /** Titles whose audio wasn't where the library expected it. */
  skipped: string[];
  playlist_file: string | null;
}

/** A singer you saved, with its own copy of the voice reference. */
export interface Persona {
  id: string;
  name: string;
  caption: string;
  bpm: number | null;
  keyscale: string | null;
  vocal_language: string | null;
  voice_path: string;
  voice_is_latent: boolean;
  /** Provenance only — the song may since have been deleted. */
  source_track_id: string | null;
  created_at: number;
}

export interface Playlist {
  id: string;
  name: string;
  created_at: number;
  /** Track ids, in playing order. */
  track_ids: string[];
}

export interface StemChoice {
  id: string;
  name: string;
}

export interface AvailableModels {
  lm: string[];
  embedding: string[];
  vae: string[];
  dit_turbo: string[];
  dit_sft: string[];
}

export type EngineState = "stopped" | "starting" | "ready" | "crashed";

export interface EngineStatus {
  state: EngineState;
  models: AvailableModels;
  models_complete: boolean;
  supports_stems: boolean;
  vae_chunk: number;
  cpu_fallback: boolean;
}

export interface GenerateOptions {
  prompt: string;
  lyrics?: string;
  duration?: number;
  instrumental?: boolean;
  /** Let the model expand the description. Also makes it re-pick the language. */
  embellish?: boolean;
  bpm?: number | null;
  keyscale?: string | null;
  timesignature?: string | null;
  vocal_language?: string | null;
  seed?: number | null;
  /** "best" (SFT, 50 steps) or "fast" (turbo, 8 steps). */
  quality?: "best" | "fast";
  /** How many takes of the same prompt, 1-4. */
  variations?: number;
  /** "mp3" or "wav24" for lossless. */
  format?: "mp3" | "wav24";
  /** Track id whose singer this song should sound like. */
  voice_from?: string | null;
  /** Id of a saved persona. Wins over voice_from when both are set. */
  persona?: string | null;
}

/** Stages the backend reports while a song is being made. */
export type Stage = "starting" | "writing" | "rendering" | "recovering" | "saving";

export interface StageEvent {
  job_id: string;
  stage: Stage;
  detail: string;
}

export type Tier = "light" | "standard" | "best";

export interface SetupInfo {
  tier: Tier;
  tier_label: string;
  tier_description: string;
  vram_mb: number | null;
  total_bytes: number;
  missing_bytes: number;
  missing_count: number;
  ready: boolean;
}

export interface SetupProgress {
  file: string;
  role: string;
  file_index: number;
  file_count: number;
  downloaded: number;
  total: number;
}
