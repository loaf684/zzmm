import { BatteryLow, RefreshCw } from "lucide-react";

import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Panel } from "@/components/ui/Panel";
import type { DetectedModsDir } from "@/types/settings";

interface DetectedInstallationsProps {
  results: DetectedModsDir[] | null;
  detecting: boolean;
  current: string | null;
  onDetect: () => void;
  onUse: (path: string) => void;
}

const SOURCE_LABEL: Record<string, string> = {
  "xxmi-launcher": "XXMI Launcher",
  xxmi: "XXMI",
  zzmi: "ZZMI",
  "mods-folder": "Mods",
};

function sourceTone(source: string): "cyan" | "acid" | "muted" {
  if (source === "xxmi-launcher") return "cyan";
  if (source === "zzmi") return "acid";
  return "muted";
}

export function DetectedInstallations({
  results,
  detecting,
  current,
  onDetect,
  onUse,
}: DetectedInstallationsProps) {
  return (
    <Panel
      eyebrow="Auto-detect"
      title="XXMI / ZZMI installs"
      description="Scans common launcher folders on every drive as well as your user app-data directories. Detection only reads folder names."
      action={
        <Button size="sm" variant="ghost" onClick={onDetect} loading={detecting}>
          <RefreshCw className="h-3.5 w-3.5" aria-hidden />
          Rescan
        </Button>
      }
    >
      {results === null ? (
        <p className="text-[11px] leading-relaxed text-zinc-500">
          {detecting
            ? "Scanning C:\\ through Z:\\ for XXMI and ZZMI folders…"
            : "No scan has run yet. Use Rescan to look for an existing install."}
        </p>
      ) : results.length === 0 ? (
        <div className="border border-dashed border-ink-600 px-4 py-6 text-center">
          <p className="font-display text-[11px] uppercase tracking-[0.2em] text-zinc-400">
            No installs detected
          </p>
          <p className="mx-auto mt-2 max-w-sm text-[11px] leading-relaxed text-zinc-600">
            ZZMM checked the usual XXMI Launcher, XXMI and ZZMI locations. Use
            Browse to select any folder manually.
          </p>
        </div>
      ) : (
        <ul className="divide-y divide-ink-800">
          {results.map((result) => {
            const isCurrent =
              (current?.trim() ?? "").toLowerCase() === result.path.toLowerCase();

            return (
              <li
                key={result.path}
                className="flex flex-wrap items-center gap-3 py-3 first:pt-0 last:pb-0"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-display text-[11px] uppercase tracking-[0.16em] text-zinc-300">
                      {result.label}
                    </span>
                    <Badge tone={sourceTone(result.source)}>
                      {SOURCE_LABEL[result.source] ?? result.source}
                    </Badge>
                    {result.modCount === 0 ? (
                      <Badge tone="muted">
                        <BatteryLow className="mr-1 h-2.5 w-2.5" aria-hidden />
                        Empty
                      </Badge>
                    ) : null}
                  </div>
                  <p
                    className="mt-1 truncate font-mono text-[10px] text-zinc-600"
                    title={result.path}
                  >
                    {result.path}
                  </p>
                  <p className="mt-1 font-mono text-[10px] text-zinc-600">
                    {result.modCount} entries · {result.disabledCount} disabled
                  </p>
                </div>

                <Button
                  size="sm"
                  variant={isCurrent ? "outline" : "primary"}
                  onClick={() => onUse(result.path)}
                  disabled={isCurrent}
                >
                  {isCurrent ? "Selected" : "Use"}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
    </Panel>
  );
}
