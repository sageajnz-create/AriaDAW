import { describe, expect, it } from "vitest";
import { activeIndexAt, estimateTiming } from "../timing";

// Field test: a real-shaped lyric sheet, run through the real estimator.
const ROBS_QUEST = [
  "[Intro]",
  "[Verse 1]",
  "Rob woke up one morning with a fright",
  "his family jewels were nowhere in sight",
  "he searched the bed, he searched the floor",
  "then checked the fridge and checked the door",
  "[Chorus]",
  "where are my testacles, where did they go",
  "left at a gig in Christchurch, I don't know",
  "[Guitar Solo]",
  "[Verse 2]",
  "he asked the doctor, he asked the priest",
  "he asked the sound man back at least",
  "three venues east",
  "[Outro]",
].join("\n");

describe("Rob's lost testacles: a journey, in production code", () => {
  const tl = estimateTiming(ROBS_QUEST, 154); // a brisk comedy-folk runtime

  it("parses the whole saga without choking", () => {
    expect(tl.entries.length).toBeGreaterThan(10);
  });

  it("gives the guitar solo room to be ridiculous", () => {
    const solo = tl.entries.find((e) => e.text === "Guitar Solo")!;
    const chorusLine = tl.entries.find((e) =>
      e.text.startsWith("where are my testacles"),
    )!;
    // A four-weight instrumental should outlast any single sung line.
    expect(solo.end - solo.start).toBeGreaterThan(chorusLine.end - chorusLine.start);
  });

  it("keeps every line of the search party in order", () => {
    const lines = tl.entries.filter((e) => e.kind === "line").map((e) => e.text);
    expect(lines[0]).toContain("Rob woke up");
    expect(lines).toContain("three venues east");
    const starts = tl.entries.map((e) => e.start);
    expect([...starts].sort((a, b) => a - b)).toEqual(starts);
  });

  it("highlights the right line mid-solo", () => {
    const solo = tl.entries.find((e) => e.text === "Guitar Solo")!;
    const mid = (solo.start + solo.end) / 2;
    expect(activeIndexAt(tl, mid)).toBe(tl.entries.indexOf(solo));
  });

  it("still holds the last line after the song ends", () => {
    expect(activeIndexAt(tl, 999)).toBe(tl.entries.length - 1);
  });
});
