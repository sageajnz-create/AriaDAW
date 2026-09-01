/** Larger-text preference. Lives on this machine only — no account, no cloud. */

const LARGE_TEXT_KEY = "aria.largeText";

export function loadLargeText(): boolean {
  try {
    return localStorage.getItem(LARGE_TEXT_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveLargeText(on: boolean): void {
  try {
    localStorage.setItem(LARGE_TEXT_KEY, on ? "1" : "0");
  } catch {
    /* private mode, full disk, etc. — the toggle still works for this session */
  }
}

export function prefersReducedMotion(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return false;
  }
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}
