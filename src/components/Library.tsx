import { useState } from "react";
import { api, audioUrl, formatDate, formatDuration } from "../api";
import type { Track } from "../types";

interface Props {
  tracks: Track[];
  onChanged: () => void;
}

export default function Library({ tracks, onChanged }: Props) {
  const [openLyrics, setOpenLyrics] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

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
              </div>

              <div className="track-actions">
                <button
                  className="btn btn-icon"
                  onClick={() => toggleFavorite(t)}
                  aria-pressed={t.favorite}
                  // Text, not just a colour or icon, so the state is never
                  // conveyed by appearance alone.
                  title={t.favorite ? "Remove from favourites" : "Add to favourites"}
                >
                  {t.favorite ? "★ Favourite" : "☆ Favourite"}
                </button>

                {t.lyrics && (
                  <button
                    className="btn btn-icon"
                    onClick={() => setOpenLyrics(lyricsOpen ? null : t.id)}
                    aria-expanded={lyricsOpen}
                    aria-controls={`lyrics-${t.id}`}
                  >
                    {lyricsOpen ? "Hide words" : "Words"}
                  </button>
                )}

                <button
                  className="btn btn-icon"
                  onClick={() => setConfirmDelete(t.id)}
                >
                  Delete
                </button>
              </div>
            </div>

            <audio controls preload="none" src={audioUrl(t.audio_path)}>
              Your browser cannot play audio.
            </audio>

            {lyricsOpen && (
              <div className="lyrics" id={`lyrics-${t.id}`}>
                {t.lyrics}
              </div>
            )}

            {confirmDelete === t.id && (
              <div className="notice notice-warn" role="alertdialog" aria-label="Confirm delete">
                <div>
                  <p>
                    <strong>Delete "{t.title}"?</strong>
                    This removes the audio file from your computer. It can't be undone.
                  </p>
                  <div className="btn-row" style={{ marginTop: 12 }}>
                    <button className="btn" onClick={() => remove(t.id)}>
                      Yes, delete it
                    </button>
                    <button className="btn" onClick={() => setConfirmDelete(null)} autoFocus>
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
