import { CheckCircle2, Info, X, XCircle } from "lucide-react";

import { cn } from "@/lib/cn";
import { useToastStore, type ToastVariant } from "@/store/toasts";

const TONES: Record<ToastVariant, string> = {
  success: "border-acid-400/30",
  error: "border-hot-500/40",
  info: "border-cyan-400/30",
};

const ICONS: Record<ToastVariant, typeof Info> = {
  success: CheckCircle2,
  error: XCircle,
  info: Info,
};

const ICON_COLORS: Record<ToastVariant, string> = {
  success: "text-acid-400",
  error: "text-hot-400",
  info: "text-cyan-300",
};

/** Fixed-position toast stack. Errors stay on screen longer than successes. */
export function ToastViewport() {
  const toasts = useToastStore((state) => state.toasts);
  const dismiss = useToastStore((state) => state.dismiss);

  return (
    <div
      aria-live="polite"
      aria-atomic="false"
      className="pointer-events-none fixed bottom-5 right-5 z-[80] flex w-[360px] max-w-[calc(100vw-2.5rem)] flex-col gap-2"
    >
      {toasts.map((item) => {
        const Icon = ICONS[item.variant];
        return (
          <div
            key={item.id}
            role={item.variant === "error" ? "alert" : "status"}
            className={cn(
              "pointer-events-auto flex animate-toast-in items-start gap-3 rounded-xl border bg-ink-900/95 p-3.5 shadow-pop backdrop-blur",
              TONES[item.variant],
            )}
          >
            <Icon
              className={cn("mt-0.5 h-4 w-4 shrink-0", ICON_COLORS[item.variant])}
              aria-hidden
            />
            <div className="min-w-0 flex-1">
              <p className="text-[12px] font-medium text-zinc-100">
                {item.title}
              </p>
              {item.message ? (
                <p className="mt-1 break-words text-[11px] leading-relaxed text-zinc-400">
                  {item.message}
                </p>
              ) : null}
            </div>
            <button
              type="button"
              onClick={() => dismiss(item.id)}
              className="-mr-1 -mt-1 rounded-md p-1 text-zinc-600 transition-colors hover:text-zinc-300"
              aria-label="Dismiss notification"
            >
              <X className="h-3.5 w-3.5" aria-hidden />
            </button>
          </div>
        );
      })}
    </div>
  );
}
