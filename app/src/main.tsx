import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

/**
 * In development, `?demo=launcher` or `?demo=ocr` installs a mocked Tauri
 * bridge before the first render, so the real UI can be opened in a browser
 * (README screenshots, visual review). The import is guarded by `DEV` and is
 * dropped from production builds.
 */
async function boot() {
  if (import.meta.env.DEV) {
    const demo = new URLSearchParams(window.location.search).get("demo");
    if (demo) {
      const { installMock } = await import("./demo/mock");
      installMock();
    }
  }

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void boot();
