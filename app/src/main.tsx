import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { applyTheme, resolveTheme, storedTheme, systemPrefersLight } from "./theme";

/**
 * In development, `?demo=launcher` or `?demo=ocr` installs a mocked Tauri
 * bridge before the first render, so the real UI can be opened in a browser
 * (README screenshots, visual review). The import is guarded by `DEV` and is
 * dropped from production builds.
 */
/**
 * Paint the saved theme before the first frame. Doing it in React would show
 * one dark frame on a light desktop, and a launcher that flashes is worse than
 * one that starts a millisecond later.
 */
applyTheme(resolveTheme(storedTheme(), systemPrefersLight()));

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
