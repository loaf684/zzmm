import { useEffect, useState } from "react";
import {
  AlertTriangle,
  ArrowRight,
  ExternalLink,
  Eye,
  Heart,
  ImageOff,
  Layers,
  Users,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { formatCount, formatDate } from "@/lib/format";
import { useModsStore } from "@/store/mods";
import type { ModSummary } from "@/types/gamebanana";

export interface BrowseStats {
  installed: number;
  characters: number;
  conflicts: number;
}

interface BrowseHeroProps {
  mod: ModSummary;
  stats: BrowseStats;
  onOpen: () => void;
}

function StatTile({
  icon: Icon,
  label,
  value,
  tone = "neutral",
}: {
  icon: LucideIcon;
  label: string;
  value: number;
  tone?: "neutral" | "hot";
}) {
  return (
    <div className="flex items-center gap-3 border border-ink-700 bg-ink-900/60 px-4 py-3">
      <Icon
        className={tone === "hot" ? "h-4 w-4 text-hot-400" : "h-4 w-4 text-zinc-600"}
        aria-hidden
      />
      <div className="min-w-0">
        <p className="font-mono text-[9px] uppercase tracking-[0.18em] text-zinc-500">
          {label}
        </p>
        <p className="font-display text-lg font-semibold leading-tight text-zinc-100">
          {value.toLocaleString()}
        </p>
      </div>
    </div>
  );
}

/** The store's landing card: the top of the feed rendered as a full-bleed
 *  backdrop with the install/library numbers beside it. */
export function BrowseHero({ mod, stats, onOpen }: BrowseHeroProps) {
  const openUrl = useModsStore((state) => state.openUrl);
  const [imageFailed, setImageFailed] = useState(false);

  useEffect(() => {
    setImageFailed(false);
  }, [mod.id, mod.thumbnailUrl]);

  const showImage = Boolean(mod.thumbnailUrl) && !imageFailed;

  return (
    <section
      aria-label="Featured mod"
      className="grid gap-3 lg:grid-cols-[minmax(0,1.75fr)_minmax(230px,1fr)]"
    >
      <div className="group relative isolate flex min-h-[320px] flex-col justify-end overflow-hidden border border-ink-700 bg-ink-900 p-6">
        {showImage ? (
          <img
            src={mod.thumbnailUrl ?? ""}
            alt=""
            className="absolute inset-0 -z-10 h-full w-full object-cover opacity-90 transition-transform duration-500 ease-smooth group-hover:scale-[1.03]"
            onError={() => setImageFailed(true)}
          />
        ) : (
          <div className="absolute inset-0 -z-10 flex items-center justify-center bg-gradient-to-br from-ink-800 via-ink-850 to-ink-900">
            <ImageOff className="h-8 w-8 text-zinc-700" aria-hidden />
          </div>
        )}
        <div className="absolute inset-0 -z-10 bg-gradient-to-t from-ink-950 via-ink-950/75 to-ink-950/5" />
        <div className="absolute inset-0 -z-10 bg-gradient-to-r from-ink-950/85 via-transparent to-transparent" />

        <p className="eyebrow text-acid-300">
          Featured // {mod.category ?? "Zenless Zone Zero"}
        </p>
        <h2 className="mt-2 max-w-[24ch] font-display text-[32px] font-bold leading-[1.02] tracking-tight text-zinc-50">
          {mod.name}
        </h2>
        <p className="mt-2 font-mono text-[10px] uppercase tracking-[0.18em] text-zinc-400">
          by {mod.author ?? "unknown"}
          {mod.subCategory ? ` · ${mod.subCategory}` : ""}
        </p>

        <div className="mt-4 flex flex-wrap items-center gap-4 text-[11px] text-zinc-400">
          <span className="inline-flex items-center gap-1.5">
            <Heart className="h-3.5 w-3.5 text-zinc-500" aria-hidden />
            {formatCount(mod.likes)}
          </span>
          <span className="inline-flex items-center gap-1.5">
            <Eye className="h-3.5 w-3.5 text-zinc-500" aria-hidden />
            {formatCount(mod.views)}
          </span>
          <span className="font-mono text-[9px] uppercase tracking-[0.16em] text-zinc-500">
            updated {formatDate(mod.dateUpdated)}
          </span>
        </div>

        <div className="mt-5 flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={onOpen}
            className="inline-flex h-9 items-center gap-2 bg-acid-400 px-4 font-display text-[10px] font-semibold uppercase tracking-[0.18em] text-ink-950 transition-colors hover:bg-acid-300"
          >
            View mod
            <ArrowRight className="h-3.5 w-3.5" aria-hidden />
          </button>
          <button
            type="button"
            onClick={() => void openUrl(mod.profileUrl)}
            className="inline-flex h-9 items-center gap-2 border border-ink-600 bg-ink-950/60 px-4 font-display text-[10px] uppercase tracking-[0.18em] text-zinc-300 transition-colors hover:border-acid-400/60 hover:text-acid-200"
          >
            GameBanana
            <ExternalLink className="h-3.5 w-3.5" aria-hidden />
          </button>
        </div>
      </div>

      <div className="grid grid-cols-3 gap-3 lg:grid-cols-1">
        <StatTile icon={Layers} label="Installed" value={stats.installed} />
        <StatTile icon={Users} label="Characters" value={stats.characters} />
        <StatTile
          icon={AlertTriangle}
          label="Conflicts"
          value={stats.conflicts}
          tone={stats.conflicts > 0 ? "hot" : "neutral"}
        />
      </div>
    </section>
  );
}