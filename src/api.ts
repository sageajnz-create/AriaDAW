import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import type { EngineStatus, GenerateOptions, Track } from "./types";

export const api = {
  engineStatus: () => invoke<EngineStatus>("engine_status"),
  startEngine: () => invoke<void>("start_engine"),
  generate: (options: GenerateOptions) => invoke<string>("generate", { options }),
  listTracks: (limit?: number) => invoke<Track[]>("list_tracks", { limit }),
  setFavorite: (id: string, favorite: boolean) =>
    invoke<void>("set_favorite", { id, favorite }),
  renameTrack: (id: string, title: string) => invoke<void>("rename_track", { id, title }),
  deleteTrack: (id: string) => invoke<void>("delete_track", { id }),
  libraryFolder: () => invoke<string>("library_folder"),
};

/** Local file path -> a URL the webview is allowed to play. */
export const audioUrl = (path: string) => convertFileSrc(path);

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
