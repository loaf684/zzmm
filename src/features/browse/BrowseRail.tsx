import { useEffect, useState } from "react";
import { ChevronRight, Eye, Heart, ImageOff, TrendingUp } from "lucide-react";

import { formatCount } from "@/lib/format";
import type { ModSummary } from "@/types/gamebanana";

interface RailCardProps {
  mod: ModSummary;
  onOpen: () => void;
}

function RailCard({ mod, onOpen }: RailCardProps) {
  const [imageFailed, setImageFailed] = useState(false);

  useEffect(() => {
    setImageFailed(false);
  }, [mod.id, mod.thumbnailUrl]);

  const showImage = Boolean(mod.thumbnailUrl) && !imageFailed;
  const label = mod.subCategory ?? mod.category;

  return (
    <button
      type="button"
      onClick={onOpen}
      className="group w-[228px] shrink-0 snap-start border border-ink-700 bg-ink-900/60 text-left transition-colors hover:border-acid-400/50 hover:bg-ink-900"
    >
      <div className="relative aspect-[4/3] overflow-hidden bg-ink-850">
        {showImage ? (
          <img
            src={mod.thumbnailUrl ?? ""}
            alt=""
            loading="lazy"
            decoding="async"
            className="h-full w-full object-cover transition-transform duration-300 ease-smooth group-hover:scale-[1.04]"
            onError={() => setImageFailed(true)}
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center bg-gradient-to-br from-ink-800 via-ink-850 to-ink-900">
            <ImageOff className="h-5 w-5 text-zinc-700" aria-hidden />
          </div>
        )}

        {label ? (
          <span className="absolute left-1.5 top-1.5 max-w-[62%] truncate rounded border border-ink-600/70 bg-ink-950/80 px-1 py-0.5 font-display text-[8px] uppercase tracking-[0.14em] text-zinc-300 backdrop-blur">
            {label}
          </span>
        ) : null}

        {mod.version ? (
          <span className="absolute right-1.5 top-1.5 rounded border border-ink-600/70 bg-ink-950/80 px-1 py-0.5 font-mono text-[9px] text-zinc-300 backdrop-blur">
            {mod.version}
          </span>
        ) : null}
      </div>
      <div className="p-3">
        <h3
          className="truncate font-display text-[12px] font-semibold tracking-wide text-zinc-100"
          title={mod.name}
        >
          {mod.name}
        </h3>
        <p className="mt-1 flex items-center gap-1.5 truncate font-mono text-[9px] uppercase tracking-[0.14em] text-zinc-500">
          <span className="truncate">{mod.author ?? "unknown"}</span>
          {!mod.hasFiles ? (
            <span className="shrink-0 rounded border border-ink-600 bg-ink-800/70 px-1 py-0.5 font-display text-[8px] uppercase tracking-[0.12em] text-zinc-500">
              no files
            </span>
          ) : null}
          {mod.nsfw === "nsfw" ? (
            <span className="shrink-0 rounded border border-hot-500/40 bg-hot-500/20 px-1 py-0.5 font-display text-[8px] uppercase tracking-[0.12em] text-hot-300">
              NSFW
            </span>
          ) : null}
        </p>
        <div className="mt-2 flex items-center gap-3 font-mono text-[10px] text-zinc-500">
          <span className="inline-flex items-center gap-1">
            <Heart className="h-3 w-3" aria-hidden />
            {formatCount(mod.likes)}
          </span>
          <span className="inline-flex items-center gap-1">
            <Eye className="h-3 w-3" aria-hidden />
            {formatCount(mod.views)}
          </span>
        </div>
      </div>
    </button>
  );
}

interface BrowseRailProps {
  /** Category name; empty renders the fallback "Trending now" heading. */
  label: string;
  items: ModSummary[];
  onOpen: (modId: number) => void;
  /** Omitted when there is no category to widen the shelf to. */
  onSeeAll?: () => void;
}

/** One horizontal shelf of real trending mods, scoped to a category when the
 *  shelf has one; the category word is the only accent so rails can sit
 *  together without competing. */
export function BrowseRail({ label, items, onOpen, onSeeAll }: BrowseRailProps) {
  return (
    <section
      className="mt-8"
      aria-label={label ? `Trending in ${label}` : "Trending now"}
    >
      <div className="flex items-end justify-between gap-3">
        <h2 className="flex items-center gap-2 font-display text-[15px] uppercase tracking-[0.16em] text-zinc-300">
          <TrendingUp className="h-3.5 w-3.5 text-acid-400" aria-hidden />
          {label ? (
            <>
              Trending in <span className="text-acid-400">{label}</span>
            </>
          ) : (
            <>
              Trending <span className="text-acid-400">now</span>
            </>
          )}
        </h2>
        {onSeeAll ? (
          <button
            type="button"
            onClick={onSeeAll}
            className="-my-2 inline-flex h-8 items-center gap-1 px-2 font-mono text-[9px] uppercase tracking-[0.18em] text-zinc-500 transition-colors hover:text-acid-300"
          >
            See all
            <ChevronRight className="h-3 w-3" aria-hidden />
          </button>
        ) : null}
      </div>
      <div className="-mx-1 mt-3 flex snap-x gap-3 overflow-x-auto px-1 pb-2 [scrollbar-width:thin]">
        {items.map((mod) => (
          <RailCard key={mod.id} mod={mod} onOpen={() => onOpen(mod.id)} />
        ))}
      </div>
    </section>
  );
}

export function BrowseRailSkeleton() {
  return (
    <div aria-hidden className="mt-8 animate-pulse">
      <div className="h-4 w-48 bg-ink-800" />
      <div className="mt-3 flex gap-3 overflow-hidden">
        {Array.from({ length: 4 }).map((_, index) => (
          <div
            key={index}
            className="w-[228px] shrink-0 border border-ink-800 bg-ink-900/50"
          >
            <div className="aspect-[4/3] bg-ink-850" />
            <div className="space-y-2 p-3">
              <div className="h-2.5 w-4/5 bg-ink-800" />
              <div className="h-2 w-2/5 bg-ink-800" />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}