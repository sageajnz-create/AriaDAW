import { describe, expect, it } from "vitest";
import {
  activeIndexAt,
  estimateTiming,
  weightLyrics,
  type LyricTimeline,
} from "../timing";

const LYRICS = [
  "[Intro]",
  "[Verse 1]",
  "short line",
  "a somewhat longer sung line here",
  "[Chorus]",
  "word word word word",
  "[Guitar Solo]",
  "[Outro]",
].join("\n");

describe("weightLyrics", () => {
  it("treats section markers as headings, not entries", () => {
    const w = weightLyrics("[Verse 1]\nhello world\n[Chorus]\nagain");
    expect(w.map((e) => e.kind)).toEqual(["line", "line"]);
    expect(w.every((e) => e.text !== "Verse 1")).toBe(true);
  });

  it("attaches lines to their current section", () => {
    const w = weightLyrics("[Verse 1]\nfirst\n[Chorus]\nsecond");
    expect(w[0].section).toBe("Verse 1");
    expect(w[1].section).toBe("Chorus");
  });

  it("marks intro/outro as short instrumentals", () => {
    const [intro] = weightLyrics("[Intro]");
    expect(intro.kind).toBe("instrumental");
  });

  it("marks solo/break/interlude as long instrumentals", () => {
    const w = weightLyrics("[Guitar Solo]");
    expect(w[0].kind).toBe("instrumental");
  });

  it("weights sung lines by word count, minimum one", () => {
    const w = weightLyrics("one two three four five\n\nx");
    expect(w.map((e) => e.weight)).toEqual([5, 1]);
  });

  it("sizes an intro at twice the average sung line", () => {
    const w = weightLyrics("[Intro]\nfour word line here");
    // avg of one 4-word line = 4; intro (short) = 2×4 = 8
    expect(w.find((e) => e.kind === "instrumental")!.weight).toBeCloseTo(8);
  });

  it("sizes a solo at four times the average sung line", () => {
    const w = weightLyrics("[Solo]\nfour word line here");
    // solo (long) = 4×4 = 16
    expect(w.find((e) => e.kind === "instrumental")!.weight).toBeCloseTo(16);
  });

  it("falls back to fixed weights when there are no sung lines", () => {
    const w = weightLyrics("[Intro]\n[Solo]");
    const [intro, solo] = w;
    // fallback avg = 3 words: intro 6, solo 12
    expect(intro.weight).toBeCloseTo(6);
    expect(solo.weight).toBeCloseTo(12);
  });
});

describe("estimateTiming", () => {
  const timeline: LyricTimeline = estimateTiming(LYRICS, 60);

  it("covers the whole duration with contiguous spans", () => {
    let t = 0;
    for (const e of timeline.entries) {
      expect(e.start).toBeCloseTo(t, 5);
      t = e.end;
    }
    expect(t).toBeCloseTo(60, 5);
  });

  it("keeps entries in document order", () => {
    const starts = timeline.entries.map((e) => e.start);
    expect([...starts].sort((a, b) => a - b)).toEqual(starts);
  });

  it("gives a solo more time than an intro", () => {
    const byText = Object.fromEntries(
      timeline.entries.map((e) => [e.text, e.end - e.start]),
    );
    expect(byText["Guitar Solo"]).toBeGreaterThan(byText["Intro"]);
    expect(byText["Intro"]).toBeGreaterThan(0);
  });

  it("clamps non-positive durations to something sane", () => {
    const tl = estimateTiming("one line only", 0);
    expect(tl.entries[0].end).toBeGreaterThan(0);
  });
});

describe("activeIndexAt", () => {
  const tl = estimateTiming("aaa\nbbb\nccc", 30);

  it("returns -1 before the first entry", () => {
    expect(activeIndexAt(tl, tl.entries[0].start - 0.01)).toBe(-1);
  });

  it("finds the containing span", () => {
    const mid = (tl.entries[1].start + tl.entries[1].end) / 2;
    expect(activeIndexAt(tl, mid)).toBe(1);
  });

  it("clamps to the last entry past the end of the song", () => {
    expect(activeIndexAt(tl, 9999)).toBe(tl.entries.length - 1);
  });

  it("returns -1 on an empty timeline", () => {
    expect(activeIndexAt({ entries: [] }, 10)).toBe(-1);
  });
});
