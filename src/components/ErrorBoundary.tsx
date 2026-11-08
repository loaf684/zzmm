import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/Button";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
  componentStack: string | null;
}

/**
 * Last line of defence for the UI. A render crash in any feature would
 * otherwise leave a blank WebView window with no way back, so the boundary
 * shows the message, keeps the stack available for bug reports, and offers a
 * reload. Backend failures never reach here: they arrive as `ApiError` values
 * and are shown as toasts or inline panels instead.
 */
export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null, componentStack: null };

  static getDerivedStateFromError(error: Error): Partial<ErrorBoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // React logs the error object itself; keep the component stack with it.
    this.setState({ componentStack: info.componentStack ?? null });
    console.error("ZZMM crashed while rendering", error, info.componentStack);
  }

  render() {
    const { error, componentStack } = this.state;

    if (!error) return this.props.children;

    return (
      <div className="flex min-h-screen items-center justify-center bg-ink-950 p-8">
        <div className="w-full max-w-xl border border-hot-500/40 bg-ink-900 p-6">
          <div className="flex items-start gap-3">
            <AlertTriangle
              className="mt-0.5 h-5 w-5 shrink-0 text-hot-400"
              aria-hidden
            />
            <div className="min-w-0">
              <p className="eyebrow text-hot-400">Unexpected error</p>
              <h1 className="mt-2 font-display text-xl font-bold text-zinc-50">
                ZZMM hit a problem while drawing this screen
              </h1>
              <p className="mt-2 text-xs leading-relaxed text-zinc-400">
                Your mods and settings are untouched — this is a rendering
                failure inside the window. Reloading restarts the UI without
                closing the app.
              </p>
            </div>
          </div>

          <div className="mt-4 border border-ink-700 bg-ink-950/60 px-3 py-2">
            <p className="break-words font-mono text-[11px] leading-relaxed text-hot-400">
              {error.message || error.name}
            </p>
          </div>

          {componentStack ? (
            <details className="mt-3 border border-ink-700/80 bg-ink-850/50 px-3 py-2">
              <summary className="cursor-pointer list-none font-mono text-[10px] text-zinc-400 [&::-webkit-details-marker]:hidden">
                Component stack
              </summary>
              <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-words font-mono text-[10px] leading-relaxed text-zinc-500">
                {componentStack.trim()}
              </pre>
            </details>
          ) : null}

          <div className="mt-5 flex flex-wrap gap-2">
            <Button variant="primary" onClick={() => window.location.reload()}>
              <RefreshCw className="h-3.5 w-3.5" aria-hidden />
              Reload window
            </Button>
            <Button
              variant="ghost"
              onClick={() => {
                this.setState({ error: null, componentStack: null });
              }}
            >
              Try rendering again
            </Button>
          </div>
        </div>
      </div>
    );
  }
}
