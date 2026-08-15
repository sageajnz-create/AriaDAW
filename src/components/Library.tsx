import { useState } from "react";
import { api, formatDate, formatDuration, trackAudioUrl } from "../api";
import { isDesktop } from "../preview";
import Derive from "./Derive";
import type { Track } from "../types";

interface Props {
  tracks: Track[];
  onChanged: () => void;
  supportsStems: boolean;
  busy: boolean;
  onDeriveStarted: (jobId: string) => void;
}

export default function Library({
  tracks, onChanged, supportsStems, busy, onDeriveStarted,
}: Props) {
  const [openLyrics, setOpenLyrics] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [playError, setPlayError] = useState<Record<string, string>>({});

  /** Turn a MediaError into something a person can act on — and something
   *  specific enough to debug from, rather than "an error".
   *
   *  Takes a nullable element on purpose: React clears `currentTarget` once the
   *  event has been dispatched, so anything reading it late gets null. */
  function describeMediaError(el: HTMLAudioElement | null): string {
    const e = el?.error;
    if (!e) return "Playback failed for an unknown reason.";
    const map: Record<number, string> = {
      1: "Playback was aborted.",
      2: "A network error stopped playback.",
      3: "This file could not be decoded. Your system may be missing an MP3 decoder.",
      4: "This audio source isn't supported by the app's player.",
    };
    return `${map[e.code] ?? "Playback failed."} (code ${e.code}${e.message ? `: ${e.message}` : ""})`;
  }

  if (tracks.length === 0) {
    return (
      <div className="empty">
        <h3>No songs yet</h3>
        <p>Everything you make appears here, and stays on your computer.</p>
      </div>
    );
  }

  async function toggleFavorite(t: Track) {
    await api.setFavorite(t.id, !t.favorite);
    onChanged();
  }

  async function remove(id: string) {
    await api.deleteTrack(id);
    setConfirmDelete(null);
    onChanged();
  }

  return (
    <ul className="tracks">
      {tracks.map((t) => {
        const lyricsOpen = openLyrics === t.id;
        const src = trackAudioUrl(t.id);
        return (
          <li key={t.id} className="track">
            <div className="track-top">
              <div>
                <h3 className="track-title">{t.title}</h3>
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

                <button type="button" className="btn btn-icon" onClick={() => setConfirmDelete(t.id)}>
                  Delete
                </button>
              </div>
            </div>

            {t.missing ? (
              <div className="notice notice-warn" style={{ marginTop: 12 }}>
                <p>
                  <strong>This song's file isn't where Aria left it</strong>
                  It was renamed, moved or deleted outside the app. That's allowed —
                  your music is yours — but Aria can't play it from here anymore.
                  Delete removes this entry from the list.
                </p>
              </div>
            ) : src ? (
              <>
                <audio
                  controls
                  preload="none"
                  src={src}
                  onError={(e) => {
                    // Read the element NOW. Passing `e.currentTarget` into the
                    // updater below defers the read until React runs it, by
                    // which point React has nulled it — that threw a TypeError
                    // during render, unmounted the whole app, and left a blank
                    // grey window that looked for all the world like a driver
                    // bug.
                    const message = describeMediaError(e.currentTarget);
                    setPlayError((p) => ({ ...p, [t.id]: message }));
                  }}
                  onPlaying={() =>
                    setPlayError((p) => {
                      const { [t.id]: _drop, ...rest } = p;
                      return rest;
                    })
                  }
                >
                  Your browser cannot play audio.
                </audio>
                {playError[t.id] && (
                  <p className="notice notice-err" role="alert" style={{ marginTop: 8 }}>
                    {playError[t.id]} The file itself is fine — open your music
                    folder to play it in another app.
                  </p>
                )}
              </>
            ) : (
              !isDesktop && (
                <p className="hint" style={{ marginTop: 12 }}>
                  Audio plays in the Aria app. This is a design preview.
                </p>
              )
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
  );
}

/** Title of the track this one was derived from, if it's still around. */
function parentTitle(tracks: Track[], parentId: string | null): string | null {
  if (!parentId) return null;
  return tracks.find((t) => t.id === parentId)?.title ?? null;
}
