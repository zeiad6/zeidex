import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { applyTheme, resolveTheme, storedTheme, systemPrefersLight } from "./theme";
import { ThemeProvider } from "./theme-context";

/**
 * In development, `?demo=launcher` or `?demo=ocr` installs a mocked Tauri
 * bridge before the first render, so the real UI can be opened in a browser
 * (README screenshots, visual review). The import is guarded by `DEV` and is
 * dropped from production builds.
 */
/**
 * Windows renders the window opaque: the shell fills it edge to edge instead
 * of floating a rounded panel over a transparent surface. WebView2 composites
 * a transparent window through the DWM blur-behind path, which is what produced
 * ghosting, see-through panels and repaint artifacts on resize. The platform is
 * recorded on the root element before the first React render so the CSS can
 * switch the shell without a frame of the wrong layout.
 */
if (typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent)) {
  document.documentElement.dataset.os = "windows";
}

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
      <ThemeProvider>
        <App />
      </ThemeProvider>
    </React.StrictMode>,
  );
}

void boot();
