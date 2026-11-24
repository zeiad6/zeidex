/**
 * A last line of defence for the window.
 *
 * React unmounts the whole tree when a render throws and nothing catches it,
 * which leaves a native window with an empty root: a black rectangle with no
 * text, no buttons and no clue. That is exactly how a single wrong field name
 * once turned the Models screen into an unresponsive black surface. This
 * boundary keeps the failure visible and recoverable: the message, the screen
 * that failed, and a way back to the launcher.
 *
 * It is deliberately class-based because `componentDidCatch` has no hook
 * equivalent, and it reports nothing anywhere: the message stays on screen and
 * in the local console. No telemetry.
 */
import { Component, type ErrorInfo, type ReactNode } from "react";
import { Icon } from "./components/Icons";

interface Props {
  /** Which surface is mounted; a change resets the boundary. */
  scope: string;
  rtl: boolean;
  onReset: () => void;
  children: ReactNode;
}

interface State {
  message: string | null;
}

export default class ErrorBoundary extends Component<Props, State> {
  state: State = { message: null };

  static getDerivedStateFromError(error: unknown): State {
    return {
      message:
        error instanceof Error
          ? error.message
          : typeof error === "string"
            ? error
            : "Unknown render error",
    };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    // Local console only; useful when the webview inspector is open.
    console.error(`[zeidex] ${this.props.scope} failed to render`, error, info.componentStack);
  }

  componentDidUpdate(previous: Props): void {
    if (previous.scope !== this.props.scope && this.state.message !== null) {
      this.setState({ message: null });
    }
  }

  render(): ReactNode {
    const { message } = this.state;
    if (message === null) return this.props.children;

    const { rtl, onReset } = this.props;
    const t = (en: string, ar: string) => (rtl ? ar : en);
    return (
      <div className="flex h-full items-center justify-center px-3 py-3">
        <div className="panel w-[min(560px,94vw)] p-6">
          <div className="flex items-center gap-3">
            <span className="grid h-9 w-9 place-items-center rounded-full border border-rose-400/30 bg-rose-500/10 text-rose-300">
              <Icon name="alert" size={16} />
            </span>
            <div className="flex flex-col leading-tight">
              <span className="text-[0.9rem] font-semibold text-white">
                {t("This screen could not be shown", "تعذّر عرض هذه الشاشة")}
              </span>
              <span className="mt-1 text-[0.7rem] text-zinc-500">
                {t(
                  "The rest of Zeidex is still running.",
                  "بقية Zeidex لا تزال تعمل.",
                )}
              </span>
            </div>
          </div>

          <pre className="mt-4 max-h-40 overflow-auto rounded-lg border border-white/10 bg-code p-3 font-mono text-[0.68rem] leading-relaxed text-zinc-300">
            {message}
          </pre>

          <div className="mt-4 flex items-center gap-2">
            <button
              type="button"
              onClick={onReset}
              className="rounded-lg border border-accent-400/40 bg-accent-500/15 px-3 py-1.5 text-[0.75rem] font-medium text-accent-200 transition hover:bg-accent-500/25"
            >
              {t("Back to launcher", "العودة إلى المشغّل")}
            </button>
            <button
              type="button"
              onClick={() => this.setState({ message: null })}
              className="rounded-lg border border-white/10 px-3 py-1.5 text-[0.75rem] text-zinc-300 transition hover:bg-white/10"
            >
              {t("Try again", "إعادة المحاولة")}
            </button>
          </div>
        </div>
      </div>
    );
  }
}
