import { useEffect, useState } from "react";
import { ArrowRight, ChevronRight, Eye, Heart, ImageOff } from "lucide-react";

import { cn } from "@/lib/cn";
import { formatCount, formatDate } from "@/lib/format";
import type { ModSummary } from "@/types/gamebanana";

export type BrowseView = "grid" | "list";

/** Anything added within this window gets a "new" flag. */
const NEW_WINDOW_SECONDS = 21 * 24 * 60 * 60;

function isFresh(dateAdded: number): boolean {
  if (!dateAdded) return false;
  return Date.now() / 1000 - dateAdded < NEW_WINDOW_SECONDS;
}

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
  const fresh = isFresh(mod.dateAdded);
  const label = mod.subCategory ?? mod.category;

  const thumbnail = showImage ? (
    <img
      src={mod.thumbnailUrl ?? ""}
      alt=""
      loading="lazy"
      decoding="async"
      className="h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.04]"
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
        className="group flex w-full items-center gap-3 border border-ink-700/90 bg-ink-900/60 p-2.5 text-left transition-colors duration-150 ease-smooth hover:border-acid-400/50 hover:bg-ink-900"
      >
        <div className="relative h-[68px] w-28 shrink-0 overflow-hidden bg-ink-850">
          {thumbnail}
          {fresh ? (
            <span className="absolute left-1 top-1 rounded border border-hot-500/40 bg-hot-500/20 px-1 py-0.5 font-display text-[8px] uppercase tracking-[0.12em] text-hot-300">
              new
            </span>
          ) : null}
          {mod.nsfw === "nsfw" ? (
            <span className="absolute right-1 top-1 rounded border border-hot-500/40 bg-hot-500/20 px-1 py-0.5 font-display text-[8px] uppercase tracking-[0.12em] text-hot-300">
              NSFW
            </span>
          ) : null}
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
            {!mod.hasFiles ? (
              <span className="shrink-0 rounded border border-ink-600 bg-ink-800/70 px-1 py-0.5 font-display text-[8px] uppercase tracking-[0.12em] text-zinc-500">
                no files
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
            {label ? (
              <span className="rounded border border-hot-500/30 bg-hot-500/[0.1] px-1.5 py-0.5 font-display uppercase tracking-[0.14em] text-hot-300">
                {label}
              </span>
            ) : null}
            {mod.category && mod.category !== label ? (
              <span className="text-zinc-600">{mod.category}</span>
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
      className="group flex flex-col overflow-hidden border border-ink-700/90 bg-ink-900/70 text-left transition-all duration-200 ease-smooth hover:-translate-y-0.5 hover:border-acid-400/50 hover:shadow-pop focus-visible:border-acid-400/50"
    >
      <div className="relative aspect-[16/9] overflow-hidden bg-ink-850">
        {thumbnail}

        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 bg-gradient-to-t from-ink-950/85 via-ink-950/5 to-transparent"
        />

        <div className="absolute left-2 top-2 flex items-center gap-1">
          {fresh ? (
            <span className="rounded-md border border-hot-500/40 bg-hot-500/20 px-1.5 py-0.5 font-display text-[8.5px] uppercase tracking-[0.14em] text-hot-300 backdrop-blur">
              new
            </span>
          ) : null}
          {!mod.hasFiles ? (
            <span className="rounded-md border border-ink-600/70 bg-ink-950/80 px-1.5 py-0.5 font-display text-[8.5px] uppercase tracking-[0.14em] text-zinc-400 backdrop-blur">
              no files
            </span>
          ) : null}
          {mod.nsfw === "nsfw" ? (
            <span className="rounded-md border border-hot-500/40 bg-hot-500/20 px-1.5 py-0.5 font-display text-[8.5px] uppercase tracking-[0.14em] text-hot-300 backdrop-blur">
              NSFW
            </span>
          ) : null}
        </div>

        {mod.version ? (
          <span className="absolute right-2 top-2 rounded-md border border-ink-600/70 bg-ink-950/80 px-1.5 py-0.5 font-mono text-[9px] text-zinc-300 backdrop-blur">
            {mod.version}
          </span>
        ) : null}

        <div className="absolute inset-x-0 bottom-0 flex items-end justify-between gap-2 p-2.5">
          <div className="flex flex-wrap items-center gap-1">
            <span className="flex items-center gap-1 rounded-md border border-ink-600/70 bg-ink-950/80 px-1.5 py-0.5 font-mono text-[9px] text-zinc-300 backdrop-blur">
              <Heart className="h-2.5 w-2.5 text-hot-400" aria-hidden />
              {formatCount(mod.likes)}
            </span>
            <span className="flex items-center gap-1 rounded-md border border-ink-600/70 bg-ink-950/80 px-1.5 py-0.5 font-mono text-[9px] text-zinc-400 backdrop-blur">
              <Eye className="h-2.5 w-2.5" aria-hidden />
              {formatCount(mod.views)}
            </span>
          </div>
          <span
            aria-hidden
            className="inline-flex h-6 w-6 shrink-0 translate-y-1 items-center justify-center rounded-md border border-acid-400/40 bg-acid-400/10 text-acid-300 opacity-0 transition-all duration-200 ease-smooth group-hover:translate-y-0 group-hover:opacity-100 group-focus-visible:translate-y-0 group-focus-visible:opacity-100"
          >
            <ArrowRight className="h-3 w-3" />
          </span>
        </div>
      </div>

      <div className="flex flex-1 flex-col gap-1.5 p-3">
        <h3
          className="line-clamp-2 font-display text-[13.5px] font-semibold leading-snug tracking-wide text-zinc-100"
          title={mod.name}
        >
          {mod.name}
        </h3>
        <p className="truncate text-[11px] text-zinc-500">
          by {mod.author ?? "unknown"}
        </p>

        <div className="mt-auto flex flex-wrap items-center gap-x-2 gap-y-1 pt-2">
          {label ? (
            <span className="whitespace-nowrap rounded border border-hot-500/30 bg-hot-500/[0.1] px-1.5 py-0.5 font-display text-[8.5px] uppercase tracking-[0.14em] text-hot-300">
              {label}
            </span>
          ) : null}
          {mod.category && mod.category !== label ? (
            <span className="whitespace-nowrap font-mono text-[9px] text-zinc-600">
              {mod.category}
            </span>
          ) : null}
          <span
            className="ml-auto shrink-0 font-mono text-[9px] text-zinc-600"
            title={`Updated ${formatDate(mod.dateUpdated)}`}
          >
            {formatDate(mod.dateUpdated)}
          </span>
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