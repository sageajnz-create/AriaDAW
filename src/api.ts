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

  audioOutputAvailable: async (): Promise<boolean> => {
    if (!isDesktop) return true;
    return (await tauri()).invoke<boolean>("audio_output_available");
  },
};

/** Local file path -> a URL the webview may play. */
export async function audioUrl(path: string): Promise<string> {
  if (!isDesktop || !path) return "";
  const { convertFileSrc } = await tauri();
  return convertFileSrc(path);
}

/** Open a folder in the system file manager. No-op outside the desktop app. */
export async function openFolder(path: string): Promise<void> {
  if (!isDesktop) return;
  const { openPath } = await import("@tauri-apps/plugin-opener");
  await openPath(path);
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
