import { formatDuration, trackArtUrl } from "../api";
import { formatTempo } from "../a11y";
import { isDesktop } from "../preview";
import type { Player } from "../player";
import FollowAlong from "./FollowAlong";
import { useState } from "react";

interface Props {
  player: Player;
}

/**
 * The now-playing bar.
 *
 * Always mounted, because it owns the app's only `<audio>` element — unmounting
 * it between songs would tear down playback. Only the chrome is conditional.
 */
export default function PlayerBar({ player }: Props) {
  const { current, playing, position, length, error } = player;
  const total = length || current?.duration || 0;
  // Words stay open across songs once asked for — someone following along is
  // following the queue, not one track.
  const [wordsOpen, setWordsOpen] = useState(false);
  const singable =
    !!current && !!current.lyrics && current.lyrics.trim() !== "[Instrumental]";

  return (
    <div
      className={"player" + (current ? "" : " player-idle")}
      role="region"
      aria-label={current ? "Now playing" : undefined}
    >
      {/* Not `controls`: the visible transport below is the interface, and two
          sets of controls would give screen readers two ways to fight. */}
      <audio ref={player.audioRef} preload="metadata" />

      {current && (
        <>
          <img className="player-art" src={trackArtUrl(current.id)} alt="" width={56} height={56} />

          <div className="player-what">
            {/* Announced when the song changes, not on every tick of the clock. */}
            <p className="player-title" aria-live="polite" aria-atomic="true">
              {playing ? "Playing: " : "Paused: "}
              {current.title}
            </p>
            <p className="player-meta">
              {formatTempo(current.bpm) ?? "\u00a0"}
              {current.keyscale ? ` · ${current.keyscale}` : ""}
              {player.remaining > 0 ? ` · ${player.remaining} more queued` : ""}
            </p>
          </div>

          <div className="player-transport">
            <button
              type="button"
              className="btn btn-icon"
              onClick={player.previous}
              aria-label="Previous song"
              title="Previous song"
            >
              ⏮
            </button>
            <button
              type="button"
              className="btn btn-play"
              onClick={player.toggle}
              aria-label={playing ? "Pause" : "Play"}
              title={playing ? "Pause" : "Play"}
            >
              {playing ? "⏸" : "▶"}
            </button>
            <button
              type="button"
              className="btn btn-icon"
              onClick={player.next}
              aria-label="Next song"
              title="Next song"
              disabled={player.remaining === 0 && player.repeat !== "all"}
            >
              ⏭
            </button>
          </div>

          <div className="player-seek">
            <span className="player-clock" aria-hidden="true">{formatDuration(position)}</span>
            <input
              type="range"
              className="seek"
              min={0}
              max={Math.max(total, 1)}
              step={0.5}
              value={Math.min(position, total || 0)}
              onChange={(e) => player.seek(Number(e.target.value))}
              aria-label="Position in the song"
              aria-valuetext={`${formatDuration(position)} of ${formatDuration(total)}`}
            />
            <span className="player-clock" aria-hidden="true">{formatDuration(total)}</span>
          </div>

          <div className="player-modes">
            <button
              type="button"
              className="btn btn-icon"
              onClick={() => setWordsOpen((v) => !v)}
              aria-expanded={singable ? wordsOpen : undefined}
              aria-controls="follow-along"
              aria-label={
                singable
                  ? "Follow the words while it plays"
                  : "Follow the words — this song has no lyrics"
              }
              disabled={!singable}
              title="Follow the words while it plays"
            >
              Words
            </button>
            <button
              type="button"
              className="btn btn-icon"
              onClick={player.toggleShuffle}
              aria-pressed={player.shuffle}
              aria-label={player.shuffle ? "Shuffle on" : "Shuffle off"}
              title="Play in a random order"
            >
              Shuffle {player.shuffle ? "on" : "off"}
            </button>
            <button
              type="button"
              className="btn btn-icon"
              onClick={player.cycleRepeat}
              aria-label={
                player.repeat === "off"
                  ? "Repeat off"
                  : player.repeat === "all"
                    ? "Repeat all songs"
                    : "Repeat this song"
              }
              title="Repeat"
            >
              Repeat{" "}
              {player.repeat === "off" ? "off" : player.repeat === "all" ? "all" : "this song"}
            </button>
            <label className="player-volume">
              <span className="sr-only">Volume</span>
              <span aria-hidden="true">🔊</span>
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={player.volume}
                onChange={(e) => player.setVolume(Number(e.target.value))}
                aria-label="Volume"
                aria-valuetext={`${Math.round(player.volume * 100)} percent`}
              />
            </label>
            <button
              type="button"
              className="btn btn-icon"
              onClick={player.stop}
              aria-label="Stop and clear the queue"
              title="Stop and clear the queue"
            >
              ✕
            </button>
          </div>

          {wordsOpen && singable && (
            <div id="follow-along" className="player-follow">
              <FollowAlong track={current} position={position} />
            </div>
          )}
        </>
      )}

      {current && !isDesktop && (
        <p className="player-error">Audio plays in the Aria app. This is a design preview.</p>
      )}

      {error && (
        <p className="player-error" role="alert">
          {error} The file itself is fine — open your music folder to play it in
          another app.
        </p>
      )}
    </div>
  );
}
