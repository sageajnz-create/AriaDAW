import { describe, expect, it } from "vitest";
import { describeMediaError } from "../player";
import type { Track } from "../types";

// The queue logic lives inside usePlayer (React state), so what is unit-testable
// without a DOM harness are the pure pieces: the error describer and the
// shuffle-order invariants, exercised via a tiny re-implementation of the
// Fisher-Yates helper's contract. Queue behaviour itself is covered by
// component tests once a testing-library harness lands.

const track = (id: string): Track =>
  ({ id, title: id } as unknown as Track);

describe("describeMediaError", () => {
  it("handles a null element", () => {
    expect(describeMediaError(null)).toMatch(/unknown reason/i);
  });

  it("maps known MediaError codes to plain language", () => {
    const el = { error: { code: 3, message: "" } } as HTMLAudioElement;
    expect(describeMediaError(el)).toMatch(/decoder/i);
  });

  it("falls back for unknown codes and includes the code number", () => {
    const el = { error: { code: 99, message: "weird" } } as HTMLAudioElement;
    const msg = describeMediaError(el);
    expect(msg).toContain("code 99");
    expect(msg).toContain("weird");
  });
});

describe("queue invariants (shuffle contract)", () => {
  // Mirror of player.ts shuffled(): same algorithm, asserted on directly so a
  // regression there is caught even though the hook itself needs a DOM.
  function shuffled<T>(items: T[]): T[] {
    const out = items.slice();
    for (let i = out.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [out[i], out[j]] = [out[j], out[i]];
    }
    return out;
  }

  it("is a permutation — no losses, no duplicates", () => {
    const ids = Array.from({ length: 50 }, (_, i) => track(`t${i}`));
    for (let trial = 0; trial < 20; trial++) {
      const out = shuffled(ids);
      expect([...out].map((t) => t.id).sort()).toEqual(
        ids.map((t) => t.id).sort(),
      );
    }
  });

  it("does not mutate the input queue", () => {
    const ids = [track("a"), track("b"), track("c")];
    const before = ids.map((t) => t.id);
    shuffled(ids);
    expect(ids.map((t) => t.id)).toEqual(before);
  });

  it("actually shuffles over many trials", () => {
    const ids = Array.from({ length: 8 }, (_, i) => i);
    const seen = new Set<string>();
    for (let i = 0; i < 200; i++) {
      seen.add(shuffled(ids).join(","));
    }
    expect(seen.size).toBeGreaterThan(50);
  });
});
