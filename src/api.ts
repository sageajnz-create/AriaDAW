import type { EngineStatus, GenerateOptions, Track } from "./types";
import { isDesktop, previewStatus, previewTracks } from "./preview";

// Tauri modules are imported lazily so a plain browser tab doesn't blow up on
// load. See preview.ts for why browser mode exists at all.
async function tauri() {
  return await import("@tauri-apps/api/core");
}

/** In-memory store standing in for the library when previewing in a browser. */
let previewLibrary: Track[] = [...previewTracks];

export const api = {
  engineStatus: async (): Promise<EngineStatus> => {
    if (!isDesktop) return previewStatus;
    return (await tauri()).invoke<EngineStatus>("engine_status");
  },

  startEngine: async (): Promise<void> => {
    if (!isDesktop) return;
    return (await tauri()).invoke<void>("start_engine");
  },

  generate: async (options: GenerateOptions): Promise<string> => {
    if (!isDesktop) return `preview-job-${Date.now()}`;
    return (await tauri()).invoke<string>("generate", { options });
  },

  listTracks: async (limit?: number): Promise<Track[]> => {
    if (!isDesktop) return previewLibrary;
    return (await tauri()).invoke<Track[]>("list_tracks", { limit });
  },

  setFavorite: async (id: string, favorite: boolean): Promise<void> => {
    if (!isDesktop) {
      previewLibrary = previewLibrary.map((t) => (t.id === id ? { ...t, favorite } : t));
      return;
    }
    return (await tauri()).invoke<void>("set_favorite", { id, favorite });
  },

  renameTrack: async (id: string, title: string): Promise<void> => {
    if (!isDesktop) {
      previewLibrary = previewLibrary.map((t) => (t.id === id ? { ...t, title } : t));
      return;
    }
    return (await tauri()).invoke<void>("rename_track", { id, title });
  },

  deleteTrack: async (id: string): Promise<void> => {
    if (!isDesktop) {
      previewLibrary = previewLibrary.filter((t) => t.id !== id);
      return;
    }
    return (await tauri()).invoke<void>("delete_track", { id });
  },

  libraryFolder: async (): Promise<string> => {
    if (!isDesktop) return "~/Music/Aria";
    return (await tauri()).invoke<string>("library_folder");
  },

  languages: async (): Promise<Array<[string, string]>> => {
    if (!isDesktop) {
      return [
        ["en", "English"], ["es", "Spanish"], ["fr", "French"],
        ["hi", "Hindi"], ["ja", "Japanese"], ["mi", "Māori"],
      ];
    }
    return (await tauri()).invoke<Array<[string, string]>>("languages");
  },

  defaultLanguage: async (): Promise<string> => {
    if (!isDesktop) return "en";
    return (await tauri()).invoke<string>("default_language");
  },

  stemChoices: async (): Promise<import("./types").StemChoice[]> => {
    if (!isDesktop) {
      return [
        { id: "vocals", name: "Vocals" },
        { id: "drums", name: "Drums" },
        { id: "bass", name: "Bass" },
        { id: "guitar", name: "Guitar" },
      ];
    }
    return (await tauri()).invoke("stem_choices");
  },

  deriveTrack: async (id: string, operation: Record<string, unknown>): Promise<string> => {
    if (!isDesktop) return `preview-derive-${Date.now()}`;
    return (await tauri()).invoke<string>("derive_track", { id, operation });
  },

  audioOutputAvailable: async (): Promise<boolean> => {
    if (!isDesktop) return true;
    return (await tauri()).invoke<boolean>("audio_output_available");
  },
};

/**
 * A playable URL for a track, served by our own `aria://` scheme.
 *
 * Windows and Android route custom schemes through an http:// host; the others
 * use the scheme directly.
 */
export function trackAudioUrl(id: string): string {
  if (!isDesktop) return "";
  const isWindowsish = navigator.userAgent.includes("Windows");
  return isWindowsish
    ? `http://aria.localhost/track/${encodeURIComponent(id)}`
    : `aria://localhost/track/${encodeURIComponent(id)}`;
}

/** Open the music folder in the system file manager. */
export async function openFolder(): Promise<void> {
  if (!isDesktop) return;
  const { invoke } = await tauri();
  await invoke<void>("open_library_folder");
}

/** Write a UI error where it can be read later. Never throws. */
export async function reportError(message: string, stack?: string): Promise<void> {
  try {
    if (!isDesktop) {
      console.error("Aria UI error:", message, stack);
      return;
    }
    const { invoke } = await tauri();
    await invoke("log_ui_error", { message, stack });
  } catch {
    /* reporting must never itself break the app */
  }
}

/** Subscribe to a backend event. Returns an unsubscribe function. */
export async function onEvent<T>(
  name: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (!isDesktop) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(name, (e) => handler(e.payload));
}

export function formatDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "0:00";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function formatDate(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}
