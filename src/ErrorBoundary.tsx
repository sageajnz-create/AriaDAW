import { Component, type ErrorInfo, type ReactNode } from "react";
import { reportError } from "./api";

/**
 * Catches render errors so a bug shows itself instead of hiding.
 *
 * Without this, React unmounts the entire tree when any component throws. The
 * window keeps running and simply goes blank — which is indistinguishable from
 * a graphics driver problem, and sent us chasing WebKit rendering settings for
 * several rounds. If the blank window is a JavaScript error, this turns it into
 * a readable message.
 */
interface State {
  error: Error | null;
  info: string | null;
}

export default class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null, info: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    this.setState({ info: info.componentStack ?? null });
    console.error("Aria UI error:", error, info);
    // Persist it: a desktop window has no console to open.
    void reportError(
      error.message || String(error),
      `${error.stack ?? ""}\n${info.componentStack ?? ""}`,
    );
  }

  render() {
    const { error, info } = this.state;
    if (!error) return this.props.children;

    return (
      <div className="app" style={{ paddingTop: 40 }}>
        <div className="notice notice-err" role="alert">
          <div>
            <p>
              <strong>Something in Aria's screen broke</strong>
              Your music is safe — this is only the window. Everything you've made
              is still in your music folder.
            </p>
            <p style={{ marginTop: 12 }}>
              <code>{error.message || String(error)}</code>
            </p>
            <div className="btn-row" style={{ marginTop: 14 }}>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => window.location.reload()}
              >
                Reload the window
              </button>
            </div>
            {info && (
              <details style={{ marginTop: 14 }}>
                <summary>Technical details</summary>
                <pre className="lyrics" style={{ marginTop: 8 }}>
                  {error.stack}
                  {info}
                </pre>
              </details>
            )}
          </div>
        </div>
      </div>
    );
  }
}
