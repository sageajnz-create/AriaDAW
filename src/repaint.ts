/**
 * Force the page to repaint when the window comes back into view.
 *
 * WebKitGTK on Linux sometimes fails to redraw after the window is hidden and
 * shown again — you switch away, come back, and the window is blank grey. The
 * app is fine underneath; only the picture is missing.
 *
 * Disabling accelerated compositing addresses the usual cause, but this is a
 * cheap safety net for whatever it misses: touching a layout-affecting property
 * forces a reflow and a fresh paint. If the window was already drawing
 * correctly, this does nothing visible.
 */
export function installRepaintOnRestore(): () => void {
  let raf = 0;

  const nudge = () => {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => {
      const root = document.getElementById("root");
      if (!root) return;
      // Reading offsetHeight between two style writes forces a synchronous
      // reflow, which is what actually triggers the repaint.
      const previous = root.style.transform;
      root.style.transform = "translateZ(0)";
      void root.offsetHeight;
      root.style.transform = previous;
    });
  };

  const onVisibility = () => {
    if (!document.hidden) nudge();
  };

  window.addEventListener("focus", nudge);
  document.addEventListener("visibilitychange", onVisibility);
  window.addEventListener("pageshow", nudge);

  return () => {
    cancelAnimationFrame(raf);
    window.removeEventListener("focus", nudge);
    document.removeEventListener("visibilitychange", onVisibility);
    window.removeEventListener("pageshow", nudge);
  };
}
