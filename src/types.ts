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
  lock_prompt?: boolean;
  bpm?: number | null;
  keyscale?: string | null;
  vocal_language?: string | null;
  seed?: number | null;
}

/** Stages the backend reports while a song is being made. */
export type Stage = "starting" | "writing" | "rendering" | "recovering" | "saving";

export interface StageEvent {
  job_id: string;
  stage: Stage;
  detail: string;
}
