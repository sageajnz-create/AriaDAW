import { useEffect, useMemo, useState } from "react";
import { api, formatDate, formatDuration, pickFolder, trackArtUrl } from "../api";
import Derive from "./Derive";
import type { Player } from "../player";
import type { ExportReport, Playlist, Track } from "../types";

interface Props {
  tracks: Track[];
  playlists: Playlist[];
  onChanged: () => void;
  onPlaylistsChanged: () => void;
  supportsStems: boolean;
  busy: boolean;
  onDeriveStarted: (jobId: string) => void;
  player: Player;
  onPersonaSaved: () => void;
}

/** "All songs" and "Favourites" are scopes too, so one control covers them all. */
type Scope = { kind: "all" } | { kind: "favourites" } | { kind: "playlist"; id: string };
type Sort = "order" | "newest" | "oldest" | "longest" | "title";

export default function Library({
  tracks, playlists, onChanged, onPlaylistsChanged,
  supportsStems, busy, onDeriveStarted, player, onPersonaSaved,
}: Props) {
  const [openLyrics, setOpenLyrics] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [addingTo, setAddingTo] = useState<string | null>(null);
  const [savingVoice, setSavingVoice] = useState<string | null>(null);
  const [voiceError, setVoiceError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exported, setExported] = useState<ExportReport | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<Scope>({ kind: "all" });
  const [sort, setSort] = useState<Sort>("newest");
  const [newName, setNewName] = useState("");
  const [makingPlaylist, setMakingPlaylist] = useState(false);
  const [renamingPlaylist, setRenamingPlaylist] = useState(false);

  const activePlaylist =
    scope.kind === "playlist" ? playlists.find((p) => p.id === scope.id) ?? null : null;

  // A playlist that gets deleted (or was never there) must not strand the view
  // on an empty scope with no way back.
  useEffect(() => {
    if (scope.kind === "playlist" && !playlists.some((p) => p.id === scope.id)) {
      setScope({ kind: "all" });
      setSort("newest");
    }
  }, [playlists, scope]);

  const visible = useMemo(() => {
    let base: Track[];
    if (scope.kind === "favourites") {
      base = tracks.filter((t) => t.favorite);
    } else if (activePlaylist) {
      // Playlist order is the playlist's, not the library's.
      const byId = new Map(tracks.map((t) => [t.id, t]));
      base = activePlaylist.track_ids
        .map((id) => byId.get(id))
        .filter((t): t is Track => !!t);
    } else {
      base = tracks;
    }

    const q = query.trim().toLowerCase();
    if (q) {
      // Searching the words as well as the title is the point: you remember a
      // line from a song far more often than what you called the file.
      base = base.filter((t) =>
        [t.title, t.prompt, t.caption, t.lyrics, t.keyscale ?? ""]
          .join(" ")
          .toLowerCase()
          .includes(q),
      );
    }

    const sorted = base.slice();
    switch (sort) {
      case "order":
        break; // already in playlist order
      case "oldest":
        sorted.sort((a, b) => a.created_at - b.created_at);
        break;
      case "longest":
        sorted.sort((a, b) => b.duration - a.duration);
        break;
      case "title":
        sorted.sort((a, b) => a.title.localeCompare(b.title));
        break;
      default:
        sorted.sort((a, b) => b.created_at - a.created_at);
    }
    return sorted;
  }, [tracks, activePlaylist, scope.kind, query, sort]);

  /** What the exported playlist file should be called. */
  function scopeName(): string {
    if (activePlaylist) return activePlaylist.name;
    if (scope.kind === "favourites") return "Favourites";
    return "Aria songs";
  }

  async function exportVisible() {
    setExportError(null);
    setExported(null);
    const dest = await pickFolder(await api.libraryFolder());
    if (!dest) return;
    setExporting(true);
    try {
      // Exports what is on screen, so a search or a filter narrows it the same
      // way it narrows everything else.
      setExported(await api.exportTracks(visible.map((t) => t.id), dest, scopeName()));
    } catch (e) {
      setExportError(String(e));
    } finally {
      setExporting(false);
    }
  }

  function chooseScope(next: Scope) {
    setScope(next);
    // A playlist is an order someone chose; respect it until they say otherwise.
    setSort(next.kind === "playlist" ? "order" : "newest");
  }

  async function toggleFavorite(t: Track) {
    await api.setFavorite(t.id, !t.favorite);
    onChanged();
  }

  async function remove(id: string) {
    // The file is about to go; leaving it loaded would only produce a decode
    // error a second later.
    if (player.current?.id === id) player.stop();
    await api.deleteTrack(id);
    setConfirmDelete(null);
    onChanged();
    onPlaylistsChanged();
  }

  async function createPlaylist(name: string) {
    const made = await api.createPlaylist(name);
    setNewName("");
    setMakingPlaylist(false);
    onPlaylistsChanged();
    chooseScope({ kind: "playlist", id: made.id });
  }

  if (tracks.length === 0) {
    return (
      <div className="empty">
        <h3>No songs yet</h3>
        <p>Everything you make appears here, and stays on your computer.</p>
      </div>
    );
  }

  return (
    <>
      <div className="lib-bar">
        <div className="chips" role="group" aria-label="Show">
          <button
            type="button"
            className="chip"
            aria-pressed={scope.kind === "all"}
            onClick={() => chooseScope({ kind: "all" })}
          >
            All songs ({tracks.length})
          </button>
          <button
            type="button"
            className="chip"
            aria-pressed={scope.kind === "favourites"}
            onClick={() => chooseScope({ kind: "favourites" })}
          >
            ★ Favourites ({tracks.filter((t) => t.favorite).length})
          </button>
          {playlists.map((p) => (
            <button
              key={p.id}
              type="button"
              className="chip"
              aria-pressed={scope.kind === "playlist" && scope.id === p.id}
              onClick={() => chooseScope({ kind: "playlist", id: p.id })}
            >
              {p.name} ({p.track_ids.length})
            </button>
          ))}
          <button
            type="button"
            className="chip chip-add"
            onClick={() => setMakingPlaylist((v) => !v)}
            aria-expanded={makingPlaylist}
          >
            + New playlist
          </button>
        </div>

        {makingPlaylist && (
          <form
            className="inline-form"
            onSubmit={(e) => {
              e.preventDefault();
              if (newName.trim()) createPlaylist(newName.trim());
            }}
          >
            <label htmlFor="new-playlist">Name this playlist</label>
            <input
              id="new-playlist"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder="Late night"
              autoFocus
            />
            <button type="submit" className="btn" disabled={!newName.trim()}>
              Create
            </button>
            <button type="button" className="btn btn-icon" onClick={() => setMakingPlaylist(false)}>
              Cancel
            </button>
          </form>
        )}

        <div className="lib-controls">
          <div className="search">
            <label htmlFor="lib-search">Search your songs</label>
            <input
              id="lib-search"
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="A word from the title, the words, or the style"
            />
          </div>

          <div className="sortby">
            <label htmlFor="lib-sort">Order</label>
            <select id="lib-sort" value={sort} onChange={(e) => setSort(e.target.value as Sort)}>
              {activePlaylist && <option value="order">Playlist order</option>}
              <option value="newest">Newest first</option>
              <option value="oldest">Oldest first</option>
              <option value="longest">Longest first</option>
              <option value="title">By title</option>
            </select>
          </div>

          <button
            type="button"
            className="btn"
            disabled={visible.length === 0}
            onClick={() => player.play(visible, visible[0]?.id ?? "")}
          >
            ▶ Play {activePlaylist ? "this playlist" : "these"}
          </button>

          <button
            type="button"
            className="btn"
            disabled={visible.length === 0 || exporting}
            onClick={exportVisible}
            title="Copy these songs somewhere else, named so you can read them"
          >
            {exporting ? "Copying…" : "Export…"}
          </button>
        </div>

        {activePlaylist && (
          <div className="playlist-admin">
            {renamingPlaylist ? (
              <form
                className="inline-form"
                onSubmit={async (e) => {
                  e.preventDefault();
                  if (!newName.trim()) return;
                  await api.renamePlaylist(activePlaylist.id, newName.trim());
                  setRenamingPlaylist(false);
                  setNewName("");
                  onPlaylistsChanged();
                }}
              >
                <label htmlFor="rename-playlist">New name for "{activePlaylist.name}"</label>
                <input
                  id="rename-playlist"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  autoFocus
                />
                <button type="submit" className="btn" disabled={!newName.trim()}>
                  Save
                </button>
                <button
                  type="button"
                  className="btn btn-icon"
                  onClick={() => setRenamingPlaylist(false)}
                >
                  Cancel
                </button>
              </form>
            ) : (
              <>
                <button
                  type="button"
                  className="btn btn-icon"
                  onClick={() => {
                    setNewName(activePlaylist.name);
                    setRenamingPlaylist(true);
                  }}
                >
                  Rename playlist
                </button>
                <button
                  type="button"
                  className="btn btn-icon"
                  onClick={async () => {
                    await api.deletePlaylist(activePlaylist.id);
                    onPlaylistsChanged();
                    chooseScope({ kind: "all" });
                  }}
                >
                  Delete playlist
                </button>
                <span className="hint">Deleting a playlist never deletes the songs in it.</span>
              </>
            )}
          </div>
        )}
      </div>

      {(exported || exportError) && (
        <div
          className={"notice " + (exportError ? "notice-err" : "notice-info")}
          role="status"
        >
          <p>
            {exportError ? (
              <>
                <strong>That export didn't finish</strong>
                {exportError}
              </>
            ) : (
              <>
                <strong>
                  Copied {exported!.written} song{exported!.written === 1 ? "" : "s"} to{" "}
                  {exported!.folder}
                </strong>
                Numbered in order, with the covers beside them
                {exported!.playlist_file ? ` and ${exported!.playlist_file}` : ""}. They're
                ordinary files now — nothing here is needed to play them.
                {exported!.skipped.length > 0 && (
                  <>
                    {" "}
                    Skipped {exported!.skipped.length} whose files weren't where Aria
                    left them: {exported!.skipped.join(", ")}.
                  </>
                )}
              </>
            )}
          </p>
          <button
            type="button"
            className="btn btn-icon"
            style={{ marginTop: 10 }}
            onClick={() => {
              setExported(null);
              setExportError(null);
            }}
          >
            Dismiss
          </button>
        </div>
      )}

      <p className="result-count" aria-live="polite">
        {visible.length === tracks.length
          ? `${tracks.length} song${tracks.length === 1 ? "" : "s"}`
          : `${visible.length} of ${tracks.length} songs`}
      </p>

      {visible.length === 0 ? (
        <div className="empty">
          <h3>Nothing here</h3>
          <p>
            {query
              ? "No song matches that search. Try a different word, or clear the search."
              : activePlaylist
                ? "This playlist is empty. Use “Add to…” on any song to put it here."
                : "No favourites yet. Star a song to keep it here."}
          </p>
        </div>
      ) : (
        <ul className="tracks">
          {visible.map((t, i) => {
            const lyricsOpen = openLyrics === t.id;
            const isCurrent = player.current?.id === t.id;
            return (
              <li key={t.id} className={"track" + (isCurrent ? " track-current" : "")}>
                <div className="track-top">
                  <button
                    type="button"
                    className="art-button"
                    onClick={() =>
                      isCurrent && player.playing ? player.toggle() : player.play(visible, t.id)
                    }
                    aria-label={
                      isCurrent && player.playing ? `Pause ${t.title}` : `Play ${t.title}`
                    }
                    disabled={t.missing}
                  >
                    <img className="art" src={trackArtUrl(t.id)} alt="" width={72} height={72} />
                    <span className="art-glyph" aria-hidden="true">
                      {isCurrent && player.playing ? "⏸" : "▶"}
                    </span>
                  </button>

                  <div className="track-what">
                    {renaming === t.id ? (
                      <form
                        className="inline-form"
                        onSubmit={async (e) => {
                          e.preventDefault();
                          const value = new FormData(e.currentTarget).get("title");
                          if (typeof value === "string" && value.trim()) {
                            await api.renameTrack(t.id, value.trim());
                            setRenaming(null);
                            onChanged();
                          }
                        }}
                      >
                        <label htmlFor={`rn-${t.id}`}>Title</label>
                        <input id={`rn-${t.id}`} name="title" defaultValue={t.title} autoFocus />
                        <button type="submit" className="btn">Save</button>
                        <button
                          type="button"
                          className="btn btn-icon"
                          onClick={() => setRenaming(null)}
                        >
                          Cancel
                        </button>
                      </form>
                    ) : (
                      <h3 className="track-title">{t.title}</h3>
                    )}
                    <p className="track-meta">
                      {formatDuration(t.duration)}
                      {t.bpm ? ` · ${t.bpm} BPM` : ""}
                      {t.keyscale ? ` · ${t.keyscale}` : ""}
                      {" · "}
                      {formatDate(t.created_at)}
                    </p>
                    {t.operation && (
                      <p className="track-lineage">
                        {parentTitle(tracks, t.parent_id) ? (
                          <>Made from <strong>{parentTitle(tracks, t.parent_id)}</strong> — {t.operation}</>
                        ) : (
                          <>{t.operation}</>
                        )}
                      </p>
                    )}
                  </div>

                  <div className="track-actions">
                    <button
                      type="button"
                      className="btn btn-icon"
                      onClick={() => toggleFavorite(t)}
                      aria-pressed={t.favorite}
                      title={t.favorite ? "Remove from favourites" : "Add to favourites"}
                    >
                      {/* Carries a word as well as a symbol, so the state is never
                          communicated by appearance alone. */}
                      {t.favorite ? "★ Favourite" : "☆ Favourite"}
                    </button>

                    <button
                      type="button"
                      className="btn btn-icon"
                      onClick={() => setAddingTo(addingTo === t.id ? null : t.id)}
                      aria-expanded={addingTo === t.id}
                      aria-controls={`add-${t.id}`}
                    >
                      Add to…
                    </button>

                    <button
                      type="button"
                      className="btn btn-icon"
                      onClick={() => {
                        setVoiceError(null);
                        setSavingVoice(savingVoice === t.id ? null : t.id);
                      }}
                      aria-expanded={savingVoice === t.id}
                      aria-controls={`voice-${t.id}`}
                      disabled={t.missing}
                      title="Keep this singer to use on other songs"
                    >
                      Save this voice
                    </button>

                    {t.lyrics && (
                      <button
                        type="button"
                        className="btn btn-icon"
                        onClick={() => setOpenLyrics(lyricsOpen ? null : t.id)}
                        aria-expanded={lyricsOpen}
                        aria-controls={`lyrics-${t.id}`}
                      >
                        {lyricsOpen ? "Hide words" : "Words"}
                      </button>
                    )}

                    <button
                      type="button"
                      className="btn btn-icon"
                      onClick={() => setRenaming(renaming === t.id ? null : t.id)}
                    >
                      Rename
                    </button>

                    <button
                      type="button"
                      className="btn btn-icon"
                      onClick={() => setConfirmDelete(t.id)}
                    >
                      Delete
                    </button>
                  </div>
                </div>

                {addingTo === t.id && (
                  <div className="add-to" id={`add-${t.id}`} role="group" aria-label="Playlists">
                    {playlists.length === 0 && (
                      <p className="hint">You haven't made a playlist yet.</p>
                    )}
                    {playlists.map((p) => {
                      const inIt = p.track_ids.includes(t.id);
                      return (
                        <button
                          key={p.id}
                          type="button"
                          className="chip"
                          aria-pressed={inIt}
                          onClick={async () => {
                            if (inIt) await api.removeFromPlaylist(p.id, t.id);
                            else await api.addToPlaylist(p.id, t.id);
                            onPlaylistsChanged();
                          }}
                        >
                          {inIt ? "✓ " : "+ "}
                          {p.name}
                        </button>
                      );
                    })}
                    <form
                      className="inline-form"
                      onSubmit={async (e) => {
                        e.preventDefault();
                        const value = new FormData(e.currentTarget).get("name");
                        if (typeof value !== "string" || !value.trim()) return;
                        const made = await api.createPlaylist(value.trim());
                        await api.addToPlaylist(made.id, t.id);
                        onPlaylistsChanged();
                        e.currentTarget.reset();
                      }}
                    >
                      <label htmlFor={`np-${t.id}`}>Or start a new one</label>
                      <input id={`np-${t.id}`} name="name" placeholder="Playlist name" />
                      <button type="submit" className="btn">Create and add</button>
                    </form>
                  </div>
                )}

                {savingVoice === t.id && (
                  <div className="add-to" id={`voice-${t.id}`}>
                    <form
                      className="inline-form"
                      onSubmit={async (e) => {
                        e.preventDefault();
                        const value = new FormData(e.currentTarget).get("name");
                        if (typeof value !== "string" || !value.trim()) return;
                        try {
                          await api.createPersona(value.trim(), t.id);
                          setSavingVoice(null);
                          setVoiceError(null);
                          onPersonaSaved();
                        } catch (err) {
                          setVoiceError(String(err));
                        }
                      }}
                    >
                      <label htmlFor={`vn-${t.id}`}>Call this voice</label>
                      <input
                        id={`vn-${t.id}`}
                        name="name"
                        defaultValue={t.title}
                        autoFocus
                      />
                      <button type="submit" className="btn">Save it</button>
                      <button
                        type="button"
                        className="btn btn-icon"
                        onClick={() => setSavingVoice(null)}
                      >
                        Cancel
                      </button>
                    </form>
                    <p className="hint">
                      Aria keeps its own copy, so this voice still works if you
                      delete the song. Pick it under <strong>Create</strong> →
                      detailed controls.
                    </p>
                    {voiceError && (
                      <p className="notice notice-err" role="alert">{voiceError}</p>
                    )}
                  </div>
                )}

                {activePlaylist && sort === "order" && (
                  <div className="reorder" role="group" aria-label={`Order within ${activePlaylist.name}`}>
                    <button
                      type="button"
                      className="btn btn-icon"
                      disabled={i === 0}
                      onClick={async () => {
                        await api.moveInPlaylist(activePlaylist.id, t.id, -1);
                        onPlaylistsChanged();
                      }}
                    >
                      ↑ Earlier
                    </button>
                    <button
                      type="button"
                      className="btn btn-icon"
                      disabled={i === visible.length - 1}
                      onClick={async () => {
                        await api.moveInPlaylist(activePlaylist.id, t.id, 1);
                        onPlaylistsChanged();
                      }}
                    >
                      ↓ Later
                    </button>
                    <button
                      type="button"
                      className="btn btn-icon"
                      onClick={async () => {
                        await api.removeFromPlaylist(activePlaylist.id, t.id);
                        onPlaylistsChanged();
                      }}
                    >
                      Remove from this playlist
                    </button>
                  </div>
                )}

                {t.missing && (
                  <div className="notice notice-warn" style={{ marginTop: 12 }}>
                    <p>
                      <strong>This song's file isn't where Aria left it</strong>
                      It was renamed, moved or deleted outside the app. That's allowed —
                      your music is yours — but Aria can't play it from here anymore.
                      Delete removes this entry from the list.
                    </p>
                  </div>
                )}

                <Derive
                  track={t}
                  supportsStems={supportsStems}
                  busy={busy}
                  onStarted={onDeriveStarted}
                />

                {lyricsOpen && (
                  <div className="lyrics" id={`lyrics-${t.id}`}>
                    {t.lyrics}
                  </div>
                )}

                {confirmDelete === t.id && (
                  <div
                    className="notice notice-warn"
                    role="alertdialog"
                    aria-label="Confirm delete"
                  >
                    <div>
                      <p>
                        <strong>Delete "{t.title}"?</strong>
                        This removes the audio file from your computer. It can't be undone.
                      </p>
                      <div className="btn-row" style={{ marginTop: 12 }}>
                        <button type="button" className="btn" onClick={() => remove(t.id)}>
                          Yes, delete it
                        </button>
                        <button
                          type="button"
                          className="btn"
                          onClick={() => setConfirmDelete(null)}
                          autoFocus
                        >
                          Keep it
                        </button>
                      </div>
                    </div>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </>
  );
}

/** Title of the track this one was derived from, if it's still around. */
function parentTitle(tracks: Track[], parentId: string | null): string | null {
  if (!parentId) return null;
  return tracks.find((t) => t.id === parentId)?.title ?? null;
}
