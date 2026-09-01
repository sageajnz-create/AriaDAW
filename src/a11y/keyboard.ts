/**
 * Keyboard movement for tablists and radiogroups.
 *
 * Arrow keys move, Home/End jump. The widgets themselves decide what "select"
 * means; this only answers "which index next".
 */

export type Move = "next" | "prev" | "first" | "last";

export function listKeyIntent(key: string): Move | null {
  switch (key) {
    case "ArrowRight":
    case "ArrowDown":
      return "next";
    case "ArrowLeft":
    case "ArrowUp":
      return "prev";
    case "Home":
      return "first";
    case "End":
      return "last";
    default:
      return null;
  }
}

export function moveIndex(current: number, length: number, intent: Move): number {
  if (length <= 0) return 0;
  const i = ((current % length) + length) % length;
  switch (intent) {
    case "next":
      return (i + 1) % length;
    case "prev":
      return (i - 1 + length) % length;
    case "first":
      return 0;
    case "last":
      return length - 1;
  }
}
