import { useEffect, useState } from "react";
import {
  ArrowRight,
  Download,
  Heart,
  ImageOff,
  Layers,
  RefreshCw,
  Sparkles,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { formatCount } from "@/lib/format";
import { parseGameBananaDescription } from "@/lib/gamebananaText";
import type { ModDetail, ModSummary } from "@/types/gamebanana";

export interface BrowseStats {
  /** `null` until the library has been scanned on this visit. */
  installed: number | null;
  /** `null` until updates have been checked this session. */
  updates: number | null;
  /** Single-slot installer: 1 while an install is running, else 0. */
  queue: number;
}

interface BrowseHeroProps {
  mod: ModSummary;
  /** Full record once loaded; the summary alone still renders the hero. */
  detail?: ModDetail;
  stats: BrowseStats;
  onOpen: () => void;
  onBrowseStore: () => void;
}

/** GameBanana descriptions are rich text; the hero only needs a short teaser. */
function teaserText(raw: string | null | undefined, limit = 240): string {
  const text = parseGameBananaDescription(raw)
    .map((segment) => segment.text)
    .join(" ")
    .replace(/\s+/g, " ")
    .trim();

  if (text.length <= limit) return text;

  const cut = text.slice(0, limit);
  const lastSpace = cut.lastIndexOf(" ");
  return `${(lastSpace > 0 ? cut.slice(0, lastSpace) : cut).trimEnd()}...`;
}

function StatTile({
  icon: Icon,
  label,
  value,
  tone = "neutral",
}: {
  icon: LucideIcon;
  label: string;
  value: number | null;
  tone?: "neutral" | "hot";
}) {
  return (
    <div className="flex items-center gap-2.5 px-0 sm:px-5 sm:first:pl-0 sm:last:pr-0">
      <Icon
        className={
          tone === "hot"
            ? "h-4 w-4 shrink-0 text-hot-400"
            : "h-4 w-4 shrink-0 text-zinc-500"
        }
        aria-hidden
      />
      <div className="min-w-0">
        <dt className="font-mono text-[9px] uppercase tracking-[0.18em] text-zinc-400">
          {label}
        </dt>
        <dd
          className="font-display text-lg font-semibold leading-tight text-zinc-100"
          title={value === null ? "Not scanned yet" : undefined}
        >
          {value === null ? "—" : value.toLocaleString()}
        </dd>
      </div>
    </div>
  );
}

/** The dashboard spotlight: the top trending mod as a full-bleed banner with
 *  the library numbers and the catalog shortcut directly beneath it. */
export function BrowseHero({
  mod,
  detail,
  stats,
  onOpen,
  onBrowseStore,
}: BrowseHeroProps) {
  const [failedSrc, setFailedSrc] = useState<string | null>(null);

  // The ~800px gallery image carries the banner; the 220px list thumbnail is
  // only ever an ambient backdrop, so the page's largest image is never a smear.
  const heroImage = detail?.gallery[0] ?? null;
  const label = mod.category ?? detail?.category ?? mod.subCategory ?? detail?.subCategory;
  const showHero = Boolean(heroImage) && failedSrc !== heroImage;
  const showBackdrop = Boolean(mod.thumbnailUrl) && failedSrc !== mod.thumbnailUrl;

  useEffect(() => {
    setFailedSrc(null);
  }, [mod.id]);

  const description = teaserText(detail?.description);

  return (
    <div className="mt-7 overflow-hidden border border-ink-700 bg-ink-900">
      <section
        aria-labelledby={`featured-${mod.id}`}
        className="group relative isolate flex min-h-[360px] flex-col justify-end overflow-hidden sm:min-h-[440px] lg:min-h-[480px]"
      >
        {showBackdrop ? (
          <img
            src={mod.thumbnailUrl ?? ""}
            alt=""
            aria-hidden
            className="absolute inset-0 -z-20 h-full w-full scale-110 object-cover blur-2xl"
            onError={() => setFailedSrc(mod.thumbnailUrl ?? null)}
          />
        ) : null}

        {showHero ? (
          <img
            src={heroImage ?? ""}
            alt=""
            className="absolute inset-0 -z-10 h-full w-full animate-fade-in object-cover transition-transform duration-700 ease-smooth group-hover:scale-[1.02]"
            onError={() => setFailedSrc(heroImage)}
          />
        ) : (
          <>
            <div className="absolute inset-0 -z-10 bg-gradient-to-br from-ink-800 via-ink-850 to-ink-900" />
            {showBackdrop ? null : (
              <div className="absolute inset-0 -z-10 flex items-center justify-center">
                <ImageOff className="h-8 w-8 text-zinc-700" aria-hidden />
              </div>
            )}
          </>
        )}
        <div className="absolute inset-0 -z-10 bg-gradient-to-t from-ink-950 via-ink-950/70 to-transparent" />
        <div className="absolute inset-0 -z-10 bg-gradient-to-r from-ink-950/90 via-ink-950/30 to-transparent" />

        <div className="w-full p-6 sm:p-8 lg:p-10">
          <p className="flex items-center gap-2 font-display text-[10px] font-medium uppercase tracking-[0.24em] text-acid-300">
            <Sparkles className="h-3.5 w-3.5" aria-hidden />
            Featured
          </p>

          <h2
            id={`featured-${mod.id}`}
            className="mt-3 max-w-[22ch] font-display text-[34px] font-bold leading-[1.02] tracking-tight text-zinc-50 sm:text-[46px] lg:text-[54px]"
          >
            {mod.name}
          </h2>

          <p className="mt-3 text-[13px] text-zinc-300">
            By <span className="text-zinc-100">{mod.author ?? "unknown"}</span>
          </p>

          <p className="mt-2.5 min-h-[2.5rem] max-w-[64ch] text-[13px] leading-relaxed text-zinc-300/90 line-clamp-2">
            {description}
          </p>

          <div className="mt-5 flex flex-wrap items-center justify-between gap-x-6 gap-y-3">
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
              {label ? (
                <span className="inline-flex items-center gap-3">
                  <span aria-hidden className="h-3.5 w-px bg-zinc-100/40" />
                  <span className="font-display text-[10px] uppercase tracking-[0.2em] text-zinc-100">
                    {label}
                  </span>
                </span>
              ) : null}
              {detail && detail.downloads > 0 ? (
                <span className="inline-flex items-center gap-1.5 font-mono text-[11px] text-zinc-300">
                  <Download className="h-3.5 w-3.5 text-zinc-400" aria-hidden />
                  {formatCount(detail.downloads)}
                </span>
              ) : null}
              <span className="inline-flex items-center gap-1.5 font-mono text-[11px] text-zinc-300">
                <Heart className="h-3.5 w-3.5 text-hot-400" aria-hidden />
                {formatCount(mod.likes)}
              </span>
            </div>

            <button
              type="button"
              onClick={onOpen}
              className="inline-flex h-9 items-center gap-2 border border-zinc-100/30 bg-ink-950/60 px-3.5 font-display text-[10px] font-semibold uppercase tracking-[0.18em] text-zinc-100 backdrop-blur transition-colors hover:border-acid-400/60 hover:text-acid-300"
            >
              View Mod
              <ArrowRight className="h-3.5 w-3.5" aria-hidden />
            </button>
          </div>
        </div>
      </section>

      <div className="flex flex-col gap-4 border-t border-ink-700 bg-ink-950 px-6 py-4 sm:flex-row sm:items-center sm:justify-between sm:px-8 lg:px-10">
        <dl className="flex flex-wrap items-center gap-y-3 sm:divide-x sm:divide-ink-700">
          <StatTile icon={Layers} label="Installed" value={stats.installed} />
          <StatTile
            icon={RefreshCw}
            label="Updates"
            value={stats.updates}
            tone={stats.updates !== null && stats.updates > 0 ? "hot" : "neutral"}
          />
          <StatTile icon={Download} label="In queue" value={stats.queue} />
        </dl>

        <button
          type="button"
          onClick={onBrowseStore}
          className="inline-flex h-9 shrink-0 items-center justify-center gap-2 rounded-md bg-zinc-100 px-4 font-display text-[10px] font-semibold uppercase tracking-[0.16em] text-ink-950 transition-colors hover:bg-white"
        >
          Browse Store
          <ArrowRight className="h-3.5 w-3.5" aria-hidden />
        </button>
      </div>
    </div>
  );
}

export function BrowseHeroSkeleton() {
  return (
    <div
      aria-hidden
      className="mt-7 animate-pulse overflow-hidden border border-ink-800 bg-ink-900/50"
    >
      <div className="min-h-[360px] bg-ink-850 sm:min-h-[440px] lg:min-h-[480px]" />
      <div className="flex flex-col gap-4 border-t border-ink-800 px-5 py-4 sm:flex-row sm:items-center sm:justify-between sm:px-6">
        <div className="flex gap-8">
          <div className="h-8 w-24 bg-ink-800" />
          <div className="h-8 w-24 bg-ink-800" />
          <div className="h-8 w-24 bg-ink-800" />
        </div>
        <div className="h-9 w-32 bg-ink-800" />
      </div>
    </div>
  );
}