import { useEffect, useMemo, useRef } from "react";
import { activeIndexAt, estimateTiming } from "../timing";
import type { Track } from "../types";

interface Props {
  track: Track;
  position: number;
}

/**
 * The words, following along.
 *
 * The highlight moves through an estimated timeline — the engine cannot say
 * when each line is sung — so the panel says so in plain language rather than
 * letting the steady movement imply precision it doesn't have. The list is a
 * focusable scrolling region so keyboard users can read at their own pace
 * instead of chasing the highlight; nothing is announced per line, which for
 * a screen reader user would be one interruption every few seconds for the
 * whole song.
 */
export default function FollowAlong({ track, position }: Props) {
  const timeline = useMemo(
    () => estimateTiming(track.lyrics, track.duration),
    [track.lyrics, track.duration],
  );
  const active = activeIndexAt(timeline, position);
  const scroller = useRef<HTMLDivElement | null>(null);

  // Keep the sung line on screen, without stealing focus or smooth-scrolling
  // past anyone's vestibular threshold: `nearest` jumps only when it must.
  useEffect(() => {
    const el = scroller.current?.querySelector(`[data-line="${active}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [active]);

  return (
    <div className="follow-along">
      <p className="follow-note">
        Words follow approximately — Aria can't hear exactly when each line is
        sung, so this is the song's shape, not a stopwatch.
      </p>
      <div
        ref={scroller}
        className="follow-scroll"
        role="group"
        aria-label={`Words in ${track.title}, approximately timed`}
        tabIndex={0}
      >
        {timeline.entries.map((entry, i) => {
          const section =
            entry.section !== null &&
            (i === 0 || timeline.entries[i - 1].section !== entry.section);
          return (
            <Fragmented key={i} show={!!section} label={entry.section}>
              <p
                data-line={i}
                className={
                  "follow-line" +
                  (entry.kind === "instrumental" ? " instrumental" : "") +
                  (i === active ? " active" : "")
                }
                aria-current={i === active ? "true" : undefined}
              >
                {entry.kind === "instrumental" ? `\u266A ${entry.text} \u266A` : entry.text}
              </p>
            </Fragmented>
          );
        })}
      </div>
    </div>
  );
}

/** A section heading rendered once, above the first line that follows it. */
function Fragmented({
  show,
  label,
  children,
}: {
  show: boolean;
  label: string | null;
  children: React.ReactNode;
}) {
  return (
    <>
      {show && label !== null && <p className="follow-section">{label}</p>}
      {children}
    </>
  );
}
