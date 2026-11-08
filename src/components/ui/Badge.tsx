import type { ReactNode } from "react";

import { cn } from "@/lib/cn";

export type BadgeTone = "acid" | "hot" | "cyan" | "muted";

const TONES: Record<BadgeTone, string> = {
  acid: "border-acid-400/30 bg-acid-400/[0.12] text-acid-300",
  hot: "border-hot-500/35 bg-hot-500/[0.12] text-hot-400",
  cyan: "border-cyan-400/25 bg-cyan-400/10 text-cyan-300",
  muted: "border-ink-600 bg-ink-800/80 text-zinc-400",
};

interface BadgeProps {
  tone?: BadgeTone;
  children: ReactNode;
  className?: string;
}

/** Small rounded status pill used on cards and in panel headers. */
export function Badge({ tone = "muted", children, className }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full border px-2 py-0.5 text-[10px] font-medium leading-4",
        TONES[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
