import { describe, expect, it } from "vitest";
import {
  AA_TEXT,
  AA_UI,
  STAGE_LABELS,
  coalesceAnnouncement,
  contrastRatio,
  deriveFinished,
  formatTempo,
  generationFinished,
  generationProgress,
  generationStarted,
  listKeyIntent,
  moveIndex,
  setupFinished,
  setupProgressKey,
  setupProgressMessage,
} from "../a11y";

describe("generation live messages", () => {
  it("announces start, each named stage, and completion in plain language", () => {
    expect(generationStarted()).toMatch(/making your song/i);
    expect(generationProgress("writing")).toBe("Writing the words and melody.");
    expect(generationProgress("rendering", "Step 3 of 8")).toBe(
      "Recording the audio. Step 3 of 8",
    );
    expect(generationFinished("Warm indie folk")).toMatch(/warm indie folk is ready/i);
    expect(generationFinished()).toMatch(/my songs/i);
    expect(deriveFinished()).toMatch(/new version is ready/i);
  });

  it("never puts a ticking elapsed clock in the announcement", () => {
    const msg = generationProgress("saving", "Writing the file");
    expect(msg).not.toMatch(/\d+s/);
    expect(msg).not.toMatch(/elapsed/i);
    expect(generationStarted()).not.toMatch(/elapsed/i);
    expect(generationFinished()).not.toMatch(/elapsed/i);
  });

  it("labels every stage the UI actually shows", () => {
    for (const stage of ["starting", "composing", "writing", "rendering", "recovering", "saving"]) {
      expect(STAGE_LABELS[stage]).toBeTruthy();
      expect(generationProgress(stage)).toContain(STAGE_LABELS[stage]);
    }
  });

  it("drops a repeat of the same sentence so progress does not stutter", () => {
    const first = generationProgress("writing");
    expect(coalesceAnnouncement(null, first)).toBe(first);
    expect(coalesceAnnouncement(first, first)).toBeNull();
    expect(coalesceAnnouncement(first, generationProgress("rendering"))).toBe(
      "Recording the audio.",
    );
  });
});

describe("setup live messages", () => {
  it("speaks the file change, not the byte counter", () => {
    const msg = setupProgressMessage("language model", 0, 4);
    expect(msg).toMatch(/language model/i);
    expect(msg).toMatch(/1 of 4/);
    expect(msg).not.toMatch(/%/);
    expect(msg).not.toMatch(/GB|MB|bytes/i);
    expect(setupFinished()).toMatch(/ready/i);
  });

  it("keys announcements on role and file, so percent ticks stay quiet", () => {
    expect(setupProgressKey("decoder", 2)).toBe("decoder:2");
    expect(setupProgressKey("decoder", 2)).toBe(setupProgressKey("decoder", 2));
    expect(setupProgressKey("decoder", 3)).not.toBe(setupProgressKey("decoder", 2));
  });
});

describe("plain language", () => {
  it("expands tempo instead of saying BPM", () => {
    expect(formatTempo(120)).toBe("120 beats per minute");
    expect(formatTempo(null)).toBeNull();
    expect(formatTempo(0)).toBeNull();
  });
});

describe("tablist / radiogroup keyboard", () => {
  it("maps arrows and Home/End", () => {
    expect(listKeyIntent("ArrowRight")).toBe("next");
    expect(listKeyIntent("ArrowLeft")).toBe("prev");
    expect(listKeyIntent("Home")).toBe("first");
    expect(listKeyIntent("End")).toBe("last");
    expect(listKeyIntent("Tab")).toBeNull();
    expect(listKeyIntent("Escape")).toBeNull();
  });

  it("wraps at both ends and jumps", () => {
    expect(moveIndex(0, 2, "next")).toBe(1);
    expect(moveIndex(1, 2, "next")).toBe(0);
    expect(moveIndex(0, 2, "prev")).toBe(1);
    expect(moveIndex(1, 3, "first")).toBe(0);
    expect(moveIndex(0, 3, "last")).toBe(2);
  });
});

/**
 * Hex values copied from `src/styles.css` `:root`. If a colour changes there,
 * change it here too — the point of the test is that the shipping palette
 * still clears AA, not that a parser can read CSS.
 */
const PALETTE = {
  bg: "#14110f",
  surface: "#1e1a17",
  text: "#f5f0ea",
  "text-dim": "#b6aca4",
  "text-faint": "#968c85",
  accent: "#e8a44c",
  "accent-ink": "#2a1c08",
  err: "#f08a7a",
  border: "#6a5f58",
  "border-strong": "#7a6e66",
};

describe("palette contrast", () => {
  it("computes the classic white-on-black ratio", () => {
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 5);
  });

  function pair(name: string, fg: keyof typeof PALETTE, bg: keyof typeof PALETTE, min: number) {
    it(`${name} is at least ${min}:1`, () => {
      expect(contrastRatio(PALETTE[fg], PALETTE[bg])).toBeGreaterThanOrEqual(min);
    });
  }

  pair("body text on background", "text", "bg", AA_TEXT);
  pair("body text on cards", "text", "surface", AA_TEXT);
  pair("secondary text on background", "text-dim", "bg", AA_TEXT);
  pair("secondary text on cards", "text-dim", "surface", AA_TEXT);
  pair("faint text on background", "text-faint", "bg", AA_TEXT);
  pair("faint text on cards", "text-faint", "surface", AA_TEXT);
  pair("accent on background", "accent", "bg", AA_TEXT);
  pair("ink on accent fills", "accent-ink", "accent", AA_TEXT);
  pair("errors on cards", "err", "surface", AA_TEXT);
  pair("control borders on cards", "border-strong", "surface", AA_UI);
  pair("card borders on background", "border", "bg", AA_UI);
  pair("focus colour on background", "accent", "bg", AA_UI);
});
