import type { ReactNode } from "react";

/** Small uppercase heading used by the installed-mod detail sections. */
export function SectionHeading({
  title,
  action,
}: {
  title: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-2">
      <h3 className="flex items-center gap-2 text-[11px] font-medium uppercase tracking-[0.14em] text-zinc-500">
        {title}
      </h3>
      {action}
    </div>
  );
}