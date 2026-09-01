import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import ErrorBoundary from "./ErrorBoundary";
import { Announcer } from "./a11y";
import { installRepaintOnRestore } from "./repaint";
import { reportError } from "./api";
import "./styles.css";

// Safety net for WebKitGTK failing to redraw after the window is hidden.
installRepaintOnRestore();

// Errors outside React's render path (event handlers, promises) would otherwise
// vanish into a console nobody can open in a desktop window. Surface them in the
// page so a blank window is never the only symptom.
function showFatal(message: string) {
  const root = document.getElementById("root");
  if (!root || root.dataset.fatal === "1") return;
  root.dataset.fatal = "1";
  root.innerHTML = `
    <div class="app" style="padding-top:40px">
      <div class="notice notice-err" role="alert">
        <div>
          <p><strong>Aria hit an error</strong>
          Your music is safe — this is only the window.</p>
          <p style="margin-top:12px"><code></code></p>
          <div class="btn-row" style="margin-top:14px">
            <button class="btn btn-primary" id="aria-reload">Reload the window</button>
          </div>
        </div>
      </div>
    </div>`;
  // Insert as text, never markup, so an error message can't inject anything.
  const code = root.querySelector("code");
  if (code) code.textContent = message;
  root.querySelector("#aria-reload")?.addEventListener("click", () => window.location.reload());
}

window.addEventListener("error", (e) => {
  void reportError(e.message || String(e.error), e.error?.stack);
  showFatal(e.message || String(e.error));
});
window.addEventListener("unhandledrejection", (e) => {
  const reason = (e as PromiseRejectionEvent).reason;
  void reportError(String(reason), reason?.stack);
  showFatal(String(reason));
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <Announcer>
        <App />
      </Announcer>
    </ErrorBoundary>
  </React.StrictMode>,
);
