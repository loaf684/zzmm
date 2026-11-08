import { useEffect, useState } from "react";
import { ChevronRight, Eye, Heart, ImageOff } from "lucide-react";

import { cn } from "@/lib/cn";
import { formatCount, formatDate } from "@/lib/format";
import type { ModSummary } from "@/types/gamebanana";

export type BrowseView = "grid" | "list";

interface BrowseCardProps {
  mod: ModSummary;
  view?: BrowseView;
  onOpen: () => void;
}

export function BrowseCard({ mod, view = "grid", onOpen }: BrowseCardProps) {
  const [imageFailed, setImageFailed] = useState(false);

  useEffect(() => {
    setImageFailed(false);
  }, [mod.id, mod.thumbnailUrl]);

  const showImage = Boolean(mod.thumbnailUrl) && !imageFailed;

  const thumbnail = showImage ? (
    <img
      src={mod.thumbnailUrl ?? ""}
      alt=""
      loading="lazy"
      decoding="async"
      className="h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.03]"
      onError={() => setImageFailed(true)}
    />
  ) : (
    <div className="flex h-full w-full items-center justify-center bg-gradient-to-br from-ink-800 via-ink-850 to-ink-900">
      <ImageOff className="h-6 w-6 text-zinc-700" aria-hidden />
    </div>
  );

  if (view === "list") {
    return (
      <button
        type="button"
        onClick={onOpen}
        className="group flex w-full items-center gap-3 border border-ink-700/90 bg-ink-900/60 p-2.5 text-left transition-colors hover:border-acid-400/50 hover:bg-ink-900"
      >
        <div className="relative h-[68px] w-28 shrink-0 overflow-hidden bg-ink-850">
          {thumbnail}
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <h3
              className="truncate font-display text-[13px] font-semibold tracking-wide text-zinc-100"
              title={mod.name}
            >
              {mod.name}
            </h3>
            {mod.version ? (
              <span className="shrink-0 font-mono text-[9px] text-zinc-500">
                {mod.version}
              </span>
            ) : null}
          </div>
          <p className="mt-0.5 truncate text-[11px] text-zinc-500">
            by {mod.author ?? "unknown"}
          </p>

          <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 font-mono text-[9px] text-zinc-600">
            <span className="flex items-center gap-1">
              <Heart className="h-2.5 w-2.5 text-hot-400" aria-hidden />
              {formatCount(mod.likes)}
            </span>
            <span className="flex items-center gap-1">
              <Eye className="h-2.5 w-2.5" aria-hidden />
              {formatCount(mod.views)}
            </span>
            <span>updated {formatDate(mod.dateUpdated)}</span>
            {mod.subCategory ?? mod.category ? (
              <span className="border border-ink-600 bg-ink-800/70 px-1.5 py-0.5 font-display uppercase tracking-[0.14em] text-zinc-400">
                {mod.subCategory ?? mod.category}
              </span>
            ) : null}
          </div>
        </div>

        <ChevronRight
          className="h-4 w-4 shrink-0 text-zinc-700 transition-colors group-hover:text-acid-300"
          aria-hidden
        />
      </button>
    );
  }

  return (
    <button
      type="button"
      onClick={onOpen}
      className="group flex flex-col overflow-hidden border border-ink-700/90 bg-ink-900/70 text-left transition-colors hover:border-acid-400/50"
    >
      <div className="relative aspect-[16/9] overflow-hidden bg-ink-850">
        {thumbnail}

        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 bg-gradient-to-t from-ink-950/80 via-transparent to-transparent"
        />

        <div className="absolute inset-x-0 bottom-0 flex items-end justify-between gap-2 p-2.5">
          <div className="flex flex-wrap items-center gap-1">
            <span className="flex items-center gap-1 border border-ink-600/70 bg-ink-950/80 px-1.5 py-0.5 font-mono text-[9px] text-zinc-300">
              <Heart className="h-2.5 w-2.5 text-hot-400" aria-hidden />
              {formatCount(mod.likes)}
            </span>
            <span className="flex items-center gap-1 border border-ink-600/70 bg-ink-950/80 px-1.5 py-0.5 font-mono text-[9px] text-zinc-400">
              <Eye className="h-2.5 w-2.5" aria-hidden />
              {formatCount(mod.views)}
            </span>
          </div>
          {mod.version ? (
            <span className="border border-ink-600/70 bg-ink-950/80 px-1.5 py-0.5 font-mono text-[9px] text-zinc-400">
              {mod.version}
            </span>
          ) : null}
        </div>
      </div>

      <div className="flex flex-1 flex-col gap-1 p-3">
        <h3
          className="line-clamp-2 font-display text-sm font-semibold leading-snug tracking-wide text-zinc-100"
          title={mod.name}
        >
          {mod.name}
        </h3>
        <p className="truncate text-[11px] text-zinc-500">
          by {mod.author ?? "unknown"}
        </p>

        <div className="mt-auto flex flex-wrap items-center gap-1 pt-2">
          {mod.subCategory ?? mod.category ? (
            <span className="border border-ink-600 bg-ink-800/70 px-1.5 py-0.5 font-display text-[9px] uppercase tracking-[0.14em] text-zinc-400">
              {mod.subCategory ?? mod.category}
            </span>
          ) : null}
          {mod.subCategory && mod.category && mod.category !== mod.subCategory ? (
            <span className="font-mono text-[9px] text-zinc-600">{mod.category}</span>
          ) : null}
        </div>
      </div>
    </button>
  );
}

interface BrowseCardSkeletonProps {
  className?: string;
}

export function BrowseCardSkeleton({ className }: BrowseCardSkeletonProps) {
  return (
    <div
      className={cn(
        "flex animate-pulse flex-col overflow-hidden border border-ink-800 bg-ink-900/50",
        className,
      )}
    >
      <div className="aspect-[16/9] bg-ink-850" />
      <div className="space-y-2 p-3">
        <div className="h-3 w-4/5 bg-ink-800" />
        <div className="h-2.5 w-2/5 bg-ink-800" />
      </div>
    </div>
  );
}
