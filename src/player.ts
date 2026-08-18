/**
 * One player for the whole app.
 *
 * The library used to give every row its own `<audio>` element, which meant
 * there was no such thing as "playing your songs" — only playing one song, and
 * then finding the next one yourself. A single element plus a queue is what
 * turns a list of files into something you can put on.
 *
 * The element is the source of truth for whether sound is coming out: `playing`
 * follows its `play`/`pause` events rather than being set optimistically, so
 * the button can never claim to be playing while the audio is stalled or was
 * paused by the system.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { trackAudioUrl } from "./api";
import type { Track } from "./types";

export type Repeat = "off" | "all" | "one";

/** How far into a song "previous" restarts it instead of going back one. */
const RESTART_WINDOW = 3;

export interface Player {
  audioRef: React.MutableRefObject<HTMLAudioElement | null>;
  current: Track | null;
  playing: boolean;
  /** Seconds elapsed, and the length we know about — metadata, else the index. */
  position: number;
  length: number;
  volume: number;
  shuffle: boolean;
  repeat: Repeat;
  error: string | null;
  /** Songs left after this one, for "3 more in Late night". */
  remaining: number;
  play(tracks: Track[], startId: string): void;
  toggle(): void;
  next(): void;
  previous(): void;
  seek(seconds: number): void;
  setVolume(v: number): void;
  toggleShuffle(): void;
  cycleRepeat(): void;
  stop(): void;
}

export function usePlayer(): Player {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  // The queue in the order it was handed to us, plus the order it plays in.
  // Keeping those separate is what lets shuffle be switched off again without
  // having lost the original sequence.
  const [queue, setQueue] = useState<Track[]>([]);
  const [order, setOrder] = useState<number[]>([]);
  const [pos, setPos] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [position, setPosition] = useState(0);
  const [length, setLength] = useState(0);
  const [volume, setVolumeState] = useState(1);
  const [shuffle, setShuffle] = useState(false);
  const [repeat, setRepeat] = useState<Repeat>("off");
  const [error, setError] = useState<string | null>(null);

  // Set when a track change should start playing on its own — a queue advance
  // or an explicit press, as opposed to merely loading a song into the bar.
  const wantPlay = useRef(false);

  const current = queue[order[pos]] ?? null;
  const remaining = Math.max(0, order.length - pos - 1);

  /* Load whatever is current, and start it if that was the intent. */
  useEffect(() => {
    const el = audioRef.current;
    if (!el) return;
    if (!current) {
      el.pause();
      el.removeAttribute("src");
      el.load();
      return;
    }
    const url = trackAudioUrl(current.id);
    if (!url) return;
    if (el.src !== url) {
      el.src = url;
      el.load();
      setPosition(0);
      setLength(current.duration || 0);
      setError(null);
    }
    if (wantPlay.current) {
      // A rejected play() is normal — a missing decoder, or the source changing
      // underneath. The `error` event carries the detail; this only stops an
      // unhandled rejection.
      el.play().catch(() => {});
    }
  }, [current]);

  useEffect(() => {
    const el = audioRef.current;
    if (el) el.volume = volume;
  }, [volume]);

  /* Element events. Re-bound as the queue moves so the handlers never close
     over a stale position. */
  useEffect(() => {
    const el = audioRef.current;
    if (!el) return;

    const onPlay = () => {
      setPlaying(true);
      setError(null);
    };
    const onPause = () => setPlaying(false);
    const onTime = () => setPosition(el.currentTime);
    const onMeta = () => {
      if (Number.isFinite(el.duration) && el.duration > 0) setLength(el.duration);
    };
    const onError = () => {
      // Read the element now, not in a deferred updater: React nulls out event
      // targets after dispatch, and reading one late is what previously threw
      // during render and blanked the window.
      setError(describeMediaError(el));
      setPlaying(false);
    };
    const onEnded = () => {
      if (repeat === "one") {
        el.currentTime = 0;
        el.play().catch(() => {});
        return;
      }
      if (pos + 1 < order.length) {
        wantPlay.current = true;
        setPos(pos + 1);
      } else if (repeat === "all" && order.length > 0) {
        wantPlay.current = true;
        setPos(0);
      } else {
        wantPlay.current = false;
        setPlaying(false);
      }
    };

    el.addEventListener("play", onPlay);
    el.addEventListener("pause", onPause);
    el.addEventListener("timeupdate", onTime);
    el.addEventListener("loadedmetadata", onMeta);
    el.addEventListener("durationchange", onMeta);
    el.addEventListener("error", onError);
    el.addEventListener("ended", onEnded);
    return () => {
      el.removeEventListener("play", onPlay);
      el.removeEventListener("pause", onPause);
      el.removeEventListener("timeupdate", onTime);
      el.removeEventListener("loadedmetadata", onMeta);
      el.removeEventListener("durationchange", onMeta);
      el.removeEventListener("error", onError);
      el.removeEventListener("ended", onEnded);
    };
  }, [pos, order.length, repeat]);

  const play = useCallback(
    (tracks: Track[], startId: string) => {
      if (tracks.length === 0) return;
      const at = Math.max(0, tracks.findIndex((t) => t.id === startId));
      const indices = tracks.map((_, i) => i);
      wantPlay.current = true;
      setQueue(tracks);
      if (shuffle) {
        // The song you clicked plays first; the rest of the queue is scrambled
        // behind it. Shuffling everything including the pick would ignore it.
        setOrder([at, ...shuffled(indices.filter((i) => i !== at))]);
        setPos(0);
      } else {
        setOrder(indices);
        setPos(at);
      }
    },
    [shuffle],
  );

  const toggle = useCallback(() => {
    const el = audioRef.current;
    if (!el || !current) return;
    if (el.paused) {
      wantPlay.current = true;
      el.play().catch(() => {});
    } else {
      wantPlay.current = false;
      el.pause();
    }
  }, [current]);

  const next = useCallback(() => {
    if (pos + 1 < order.length) {
      wantPlay.current = true;
      setPos(pos + 1);
    } else if (repeat === "all" && order.length > 0) {
      wantPlay.current = true;
      setPos(0);
    }
  }, [pos, order.length, repeat]);

  const previous = useCallback(() => {
    const el = audioRef.current;
    // Standard transport behaviour: once you're into a song, "previous" means
    // "start this one again".
    if (el && el.currentTime > RESTART_WINDOW) {
      el.currentTime = 0;
      return;
    }
    if (pos > 0) {
      wantPlay.current = true;
      setPos(pos - 1);
    } else if (el) {
      el.currentTime = 0;
    }
  }, [pos]);

  const seek = useCallback((seconds: number) => {
    const el = audioRef.current;
    if (!el) return;
    el.currentTime = seconds;
    setPosition(seconds);
  }, []);

  const setVolume = useCallback((v: number) => setVolumeState(clamp01(v)), []);

  const toggleShuffle = useCallback(() => {
    setShuffle((on) => {
      const nowOn = !on;
      // Re-plan the rest of the queue around whatever is playing, so toggling
      // shuffle never interrupts the current song.
      setOrder((ord) => {
        if (ord.length === 0) return ord;
        const currentIndex = ord[pos];
        if (nowOn) {
          const rest = ord.filter((_, i) => i !== pos);
          setPos(0);
          return [currentIndex, ...shuffled(rest)];
        }
        const natural = ord.slice().sort((a, b) => a - b);
        setPos(Math.max(0, natural.indexOf(currentIndex)));
        return natural;
      });
      return nowOn;
    });
  }, [pos]);

  const cycleRepeat = useCallback(() => {
    setRepeat((r) => (r === "off" ? "all" : r === "all" ? "one" : "off"));
  }, []);

  const stop = useCallback(() => {
    wantPlay.current = false;
    audioRef.current?.pause();
    setQueue([]);
    setOrder([]);
    setPos(0);
  }, []);

  return {
    audioRef, current, playing, position, length, volume, shuffle, repeat,
    error, remaining,
    play, toggle, next, previous, seek, setVolume, toggleShuffle, cycleRepeat, stop,
  };
}

/** Turn a MediaError into something a person can act on. */
export function describeMediaError(el: HTMLAudioElement | null): string {
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

function shuffled<T>(items: T[]): T[] {
  const out = items.slice();
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}
