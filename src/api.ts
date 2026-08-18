import type {
  EngineStatus, ExportReport, GenerateOptions, Persona, Playlist, Track,
} from "./types";
import { isDesktop, previewArt, previewStatus, previewTracks } from "./preview";

// Tauri modules are imported lazily so a plain browser tab doesn't blow up on
// load. See preview.ts for why browser mode exists at all.
async function tauri() {
  return await import("@tauri-apps/api/core");
}

/** In-memory store standing in for the library when previewing in a browser. */
let previewLibrary: Track[] = [...previewTracks];
let previewPersonas: Persona[] = [
  {
    id: "preview-persona",
    name: "The folk singer",
    caption: "A warm and intimate indie folk track built around a clean, fingerpicked guitar.",
    bpm: 158, keyscale: "B♭ major", vocal_language: "en",
    voice_path: "", voice_is_latent: true,
    source_track_id: "preview-1", created_at: 0,
  },
];
let previewPlaylists: Playlist[] = [
  { id: "preview-pl", name: "Late night", created_at: 0, track_ids: ["preview-2"] },
];

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

  /** Bring an existing recording in. Suno's free tier can't do this at all. */
  importAudio: async (path: string): Promise<string> => {
    if (!isDesktop) return `preview-import-${Date.now()}`;
    return (await tauri()).invoke<string>("import_audio", { path });
  },

  setupInfo: async (tier?: string): Promise<import("./types").SetupInfo> => {
    if (!isDesktop) {
      // First-run setup is, by design, seen once. `?setup` in the browser
      // preview forces it so the screen can be reviewed without wiping models.
      const forced = new URLSearchParams(location.search).has("setup");
      const t = (tier as "light" | "standard" | "best") ?? "standard";
      return {
        tier: t, tier_label: t[0].toUpperCase() + t.slice(1),
        tier_description: t === "light"
          ? "Works on almost any computer, including without a graphics card. Songs take longer and the words are simpler."
          : t === "best"
            ? "The best words and the fullest sound. Needs a graphics card with 8 GB or more."
            : "A good balance. Needs a graphics card with about 6 GB.",
        vram_mb: 8176,
        total_bytes: 8.5e9,
        missing_bytes: forced ? 8.5e9 : 0,
        missing_count: forced ? 5 : 0,
        ready: !forced,
      };
    }
    return (await tauri()).invoke("setup_info", { tier });
  },

  downloadModels: async (tier: string): Promise<void> => {
    if (!isDesktop) return;
    return (await tauri()).invoke("download_models", { tier });
  },

  /** A new song in the style of an existing one. */
  remakeLike: async (id: string, keepWords: boolean): Promise<string> => {
    if (!isDesktop) return `preview-remake-${Date.now()}`;
    return (await tauri()).invoke<string>("remake_like", { id, keepWords });
  },

  /** Copy songs out under readable names, with covers and an .m3u. */
  exportTracks: async (
    ids: string[],
    dest: string,
    playlistName: string | null,
  ): Promise<ExportReport> => {
    if (!isDesktop) {
      return { folder: dest, written: ids.length, skipped: [], playlist_file: "Preview.m3u" };
    }
    return (await tauri()).invoke<ExportReport>("export_tracks", { ids, dest, playlistName });
  },

  listPersonas: async (): Promise<Persona[]> => {
    if (!isDesktop) return previewPersonas;
    return (await tauri()).invoke<Persona[]>("list_personas");
  },

  /** Save a track's singer under a name. The reference is copied, so the
   *  persona keeps working if that song is later deleted. */
  createPersona: async (name: string, trackId: string): Promise<Persona> => {
    if (!isDesktop) {
      const made: Persona = {
        id: `persona-${Date.now()}`, name, caption: "", bpm: null, keyscale: null,
        vocal_language: null, voice_path: "", voice_is_latent: true,
        source_track_id: trackId, created_at: 0,
      };
      previewPersonas = [...previewPersonas, made];
      return made;
    }
    return (await tauri()).invoke<Persona>("create_persona", { name, trackId });
  },

  renamePersona: async (id: string, name: string): Promise<void> => {
    if (!isDesktop) {
      previewPersonas = previewPersonas.map((p) => (p.id === id ? { ...p, name } : p));
      return;
    }
    return (await tauri()).invoke<void>("rename_persona", { id, name });
  },

  deletePersona: async (id: string): Promise<void> => {
    if (!isDesktop) {
      previewPersonas = previewPersonas.filter((p) => p.id !== id);
      return;
    }
    return (await tauri()).invoke<void>("delete_persona", { id });
  },

  listPlaylists: async (): Promise<Playlist[]> => {
    if (!isDesktop) return previewPlaylists;
    return (await tauri()).invoke<Playlist[]>("list_playlists");
  },

  createPlaylist: async (name: string): Promise<Playlist> => {
    if (!isDesktop) {
      const made = { id: `pl-${Date.now()}`, name, created_at: 0, track_ids: [] };
      previewPlaylists = [...previewPlaylists, made];
      return made;
    }
    return (await tauri()).invoke<Playlist>("create_playlist", { name });
  },

  renamePlaylist: async (id: string, name: string): Promise<void> => {
    if (!isDesktop) {
      previewPlaylists = previewPlaylists.map((p) => (p.id === id ? { ...p, name } : p));
      return;
    }
    return (await tauri()).invoke<void>("rename_playlist", { id, name });
  },

  deletePlaylist: async (id: string): Promise<void> => {
    if (!isDesktop) {
      previewPlaylists = previewPlaylists.filter((p) => p.id !== id);
      return;
    }
    return (await tauri()).invoke<void>("delete_playlist", { id });
  },

  addToPlaylist: async (playlistId: string, trackId: string): Promise<void> => {
    if (!isDesktop) {
      previewPlaylists = previewPlaylists.map((p) =>
        p.id === playlistId && !p.track_ids.includes(trackId)
          ? { ...p, track_ids: [...p.track_ids, trackId] }
          : p,
      );
      return;
    }
    return (await tauri()).invoke<void>("add_to_playlist", { playlistId, trackId });
  },

  removeFromPlaylist: async (playlistId: string, trackId: string): Promise<void> => {
    if (!isDesktop) {
      previewPlaylists = previewPlaylists.map((p) =>
        p.id === playlistId ? { ...p, track_ids: p.track_ids.filter((t) => t !== trackId) } : p,
      );
      return;
    }
    return (await tauri()).invoke<void>("remove_from_playlist", { playlistId, trackId });
  },

  /** `delta` is -1 to move a song earlier in the playlist, 1 for later. */
  moveInPlaylist: async (playlistId: string, trackId: string, delta: number): Promise<void> => {
    if (!isDesktop) {
      previewPlaylists = previewPlaylists.map((p) => {
        if (p.id !== playlistId) return p;
        const at = p.track_ids.indexOf(trackId);
        const to = at + delta;
        if (at < 0 || to < 0 || to >= p.track_ids.length) return p;
        const ids = [...p.track_ids];
        [ids[at], ids[to]] = [ids[to], ids[at]];
        return { ...p, track_ids: ids };
      });
      return;
    }
    return (await tauri()).invoke<void>("move_in_playlist", { playlistId, trackId, delta });
  },

  audioOutputAvailable: async (): Promise<boolean> => {
    if (!isDesktop) return true;
    return (await tauri()).invoke<boolean>("audio_output_available");
  },
};

/**
 * Base URL of the local audio server, fetched once and cached.
 *
 * Audio is served over loopback HTTP rather than a custom scheme: WebKitGTK
 * hands media URLs to GStreamer, which has no URI handler for custom schemes,
 * so `aria://` was fetched successfully and then never decoded.
 */
let audioBase: string | null = null;

export async function loadAudioBase(): Promise<void> {
  if (!isDesktop || audioBase) return;
  const { invoke } = await tauri();
  audioBase = await invoke<string>("audio_base_url");
}

/** A playable URL for a track. Empty until loadAudioBase() has run. */
export function trackAudioUrl(id: string): string {
  if (!isDesktop || !audioBase) return "";
  return `${audioBase}/track/${encodeURIComponent(id)}`;
}

/**
 * The track's cover art. Drawn by the backend from the track id, so it is
 * stable forever and costs no GPU time; the same picture is written beside the
 * audio file as an ordinary SVG the user keeps.
 */
export function trackArtUrl(id: string): string {
  if (!isDesktop) return previewArt(id);
  if (!audioBase) return "";
  return `${audioBase}/art/${encodeURIComponent(id)}`;
}

/** Ask for an audio file to import. Returns null if the user cancels. */
export async function pickAudioFile(): Promise<string | null> {
  if (!isDesktop) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: false,
    filters: [{ name: "Audio", extensions: ["mp3", "wav", "flac", "ogg", "m4a", "opus"] }],
  });
  return typeof picked === "string" ? picked : null;
}

/** Ask where to put an export. Returns null if the user cancels. */
export async function pickFolder(defaultPath?: string): Promise<string | null> {
  // The preview has no file dialogs; a stand-in path keeps the flow reviewable.
  if (!isDesktop) return "~/Music/Aria exports";
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, defaultPath });
  return typeof picked === "string" ? picked : null;
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
