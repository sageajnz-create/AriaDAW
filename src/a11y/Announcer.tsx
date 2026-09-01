import {
  createContext, useCallback, useContext, useEffect, useRef, useState,
  type ReactNode,
} from "react";

type Announce = (message: string) => void;

const AnnounceContext = createContext<Announce>(() => {});

/**
 * One polite live region for the whole window.
 *
 * Must stay mounted across tab changes: Create unmounts as soon as a song
 * lands in My songs, and a live region that unmounts with it never speaks
 * "ready". Visual progress stays where it is; this node is screen-reader-only
 * so the ticking elapsed timer is never part of the announcement.
 */
export function Announcer({ children }: { children: ReactNode }) {
  const [message, setMessage] = useState("");
  const timer = useRef<number>(0);

  const announce = useCallback<Announce>((text) => {
    const next = text.trim();
    if (!next) return;
    window.clearTimeout(timer.current);
    // Clearing first is what lets the same sentence be spoken twice in a row
    // (two songs in one sitting, two files in setup).
    setMessage("");
    timer.current = window.setTimeout(() => setMessage(next), 40);
  }, []);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  return (
    <AnnounceContext.Provider value={announce}>
      <div
        id="aria-live-polite"
        className="sr-only"
        role="status"
        aria-live="polite"
        aria-atomic="true"
      >
        {message}
      </div>
      {children}
    </AnnounceContext.Provider>
  );
}

export function useAnnounce(): Announce {
  return useContext(AnnounceContext);
}
