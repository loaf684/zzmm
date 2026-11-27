import { useState } from "react";
import { ChevronRight, Eye, Heart, ImageOff } from "lucide-react";

import { formatCount } from "@/lib/format";
import type { ModSummary } from "@/types/gamebanana";

interface RailCardProps {
  mod: ModSummary;
  onOpen: () => void;
}

function RailCard({ mod, onOpen }: RailCardProps) {
  const [imageFailed, setImageFailed] = useState(false);
  const showImage = Boolean(mod.thumbnailUrl) && !imageFailed;

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
      </div>
      <div className="p-3">
        <h3
          className="truncate font-display text-[12px] font-semibold tracking-wide text-zinc-100"
          title={mod.name}
        >
          {mod.name}
        </h3>
        <p className="mt-1 truncate font-mono text-[9px] uppercase tracking-[0.14em] text-zinc-500">
          {mod.author ?? "unknown"}
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
  title: string;
  items: ModSummary[];
  /** True while the category list is missing, so "See all" cannot resolve a
   *  real category filter yet. */
  seeAllDisabled?: boolean;
  onOpen: (modId: number) => void;
  onSeeAll: () => void;
}

/** One horizontal shelf of the store feed; the category word is the only
 *  accent so several rails can sit together without competing. */
export function BrowseRail({
  title,
  items,
  seeAllDisabled = false,
  onOpen,
  onSeeAll,
}: BrowseRailProps) {
  return (
    <section className="mt-8" aria-label={`Trending in ${title}`}>
      <div className="flex items-end justify-between gap-3">
        <h2 className="font-display text-[15px] uppercase tracking-[0.16em] text-zinc-300">
          Trending in <span className="text-acid-400">{title}</span>
        </h2>
        <button
          type="button"
          onClick={onSeeAll}
          disabled={seeAllDisabled}
          title={
            seeAllDisabled
              ? "Category filter is unavailable until the category list loads"
              : undefined
          }
          className="inline-flex items-center gap-1 font-mono text-[9px] uppercase tracking-[0.18em] text-zinc-500 transition-colors hover:text-acid-300 disabled:cursor-not-allowed disabled:text-zinc-700 disabled:hover:text-zinc-700"
        >
          See all
          <ChevronRight className="h-3 w-3" aria-hidden />
        </button>
      </div>
      <div className="-mx-1 mt-3 flex snap-x gap-3 overflow-x-auto px-1 pb-2 [scrollbar-width:thin]">
        {items.map((mod) => (
          <RailCard key={mod.id} mod={mod} onOpen={() => onOpen(mod.id)} />
        ))}
      </div>
    </section>
  );
}