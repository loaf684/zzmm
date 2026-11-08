import type { ReactNode } from "react";

import { cn } from "@/lib/cn";

interface PanelProps {
  eyebrow?: string;
  title?: string;
  description?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
  contentClassName?: string;
}

/** Rounded surface block used across all feature pages. */
export function Panel({
  eyebrow,
  title,
  description,
  action,
  children,
  className,
  contentClassName,
}: PanelProps) {
  const hasHeader = Boolean(eyebrow || title || action);

  return (
    <section
      className={cn("rounded-xl border border-ink-700/70 bg-ink-900", className)}
    >
      {hasHeader ? (
        <header className="flex flex-wrap items-start justify-between gap-4 border-b border-ink-700/60 px-5 py-4">
          <div className="min-w-0">
            {eyebrow ? <p className="eyebrow mb-1.5">{eyebrow}</p> : null}
            {title ? (
              <h2 className="font-display text-[15px] font-semibold leading-snug text-zinc-100">
                {title}
              </h2>
            ) : null}
            {description ? (
              <div className="mt-1.5 max-w-2xl text-[12px] leading-relaxed text-zinc-500">
                {description}
              </div>
            ) : null}
          </div>
          {action}
        </header>
      ) : null}

      <div className={cn("px-5 py-5", contentClassName)}>{children}</div>
    </section>
  );
}
