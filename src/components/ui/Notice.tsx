import type { ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Info, X } from "lucide-react";

import { cn } from "@/lib/cn";

type NoticeTone = "error" | "warning" | "info" | "success";

const TONES: Record<
  NoticeTone,
  { frame: string; icon: string; title: string }
> = {
  error: {
    frame: "border-hot-500/35 bg-hot-500/[0.06]",
    icon: "text-hot-400",
    title: "text-hot-300",
  },
  warning: {
    frame: "border-amber-400/35 bg-amber-400/[0.05]",
    icon: "text-amber-300",
    title: "text-amber-200",
  },
  info: {
    frame: "border-cyan-400/25 bg-cyan-400/[0.04]",
    icon: "text-cyan-300",
    title: "text-cyan-200",
  },
  success: {
    frame: "border-acid-400/30 bg-acid-400/[0.05]",
    icon: "text-acid-400",
    title: "text-acid-300",
  },
};

const ICONS = {
  error: AlertTriangle,
  warning: AlertTriangle,
  info: Info,
  success: CheckCircle2,
};

interface NoticeProps {
  tone?: NoticeTone;
  title?: string;
  message: ReactNode;
  action?: ReactNode;
  onDismiss?: () => void;
  className?: string;
}

/** Compact inline message. Dismissible so a failed background request cannot
 *  sit in front of the page forever. */
export function Notice({
  tone = "error",
  title,
  message,
  action,
  onDismiss,
  className,
}: NoticeProps) {
  const styles = TONES[tone];
  const Icon = ICONS[tone];

  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      className={cn(
        "flex items-start gap-2.5 rounded-xl border px-3.5 py-3",
        styles.frame,
        className,
      )}
    >
      <Icon className={cn("mt-0.5 h-3.5 w-3.5 shrink-0", styles.icon)} aria-hidden />
      <div className="min-w-0 flex-1">
        {title ? (
          <p className={cn("text-[12px] font-medium", styles.title)}>{title}</p>
        ) : null}
        <div
          className={cn(
            "break-words text-[11px] leading-relaxed text-zinc-400",
            title && "mt-0.5",
          )}
        >
          {message}
        </div>
        {action ? <div className="mt-2">{action}</div> : null}
      </div>
      {onDismiss ? (
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss message"
          className="-mr-1 -mt-1 rounded-md p-1 text-zinc-600 transition-colors hover:bg-white/5 hover:text-zinc-300"
        >
          <X className="h-3.5 w-3.5" aria-hidden />
        </button>
      ) : null}
    </div>
  );
}
