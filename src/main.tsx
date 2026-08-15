import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { installRepaintOnRestore } from "./repaint";
import "./styles.css";

// Safety net for WebKitGTK failing to redraw after the window is hidden.
installRepaintOnRestore();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
