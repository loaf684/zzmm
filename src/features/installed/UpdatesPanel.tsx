import { useEffect } from "react";
import {
  ArrowDownToLine,
  CheckCircle2,
  Loader2,
  RefreshCw,
  X,
} from "lucide-react";

import { Button } from "@/components/ui/Button";
import { Panel } from "@/components/ui/Panel";
import { cn } from "@/lib/cn";
import { useDownloadStore } from "@/store/download";
import { useUpdatesStore } from "@/store/updates";
import type { UpdateInfo } from "@/types/gamebanana";

interface UpdatesPanelProps {
  /** False while no mods folder is linked; the panel stays hidden. */
  enabled: boolean;
}

/**
 * Compares the `.zzmm.json` file IDs of GameBanana-installed mods with the
 * latest file on the site. Updating downloads the new file, installs it as a new
 * folder and disables the old one (nothing is deleted).
 */
export function UpdatesPanel({ enabled }: UpdatesPanelProps) {
  const result = useUpdatesStore((state) => state.result);
  const isChecking = useUpdatesStore((state) => state.isChecking);
  const error = useUpdatesStore((state) => state.error);
  const check = useUpdatesStore((state) => state.check);

  const jobs = useDownloadStore((state) => state.jobs);
  const install = useDownloadStore((state) => state.install);
  const cancel = useDownloadStore((state) => state.cancel);

  // One background check per session, not on every render.
  useEffect(() => {
    if (!enabled) return;
    const store = useUpdatesStore.getState();
    if (!store.result && !store.isChecking) void store.check();
  }, [enabled]);

  if (!enabled || (!result && !isChecking && !error)) return null;

  const updates = result?.updates.filter((item) => item.hasUpdate) ?? [];
  const failures = result?.updates.filter((item) => item.error) ?? [];

  const updateOne = async (item: UpdateInfo) => {
    const installed = await install({
      modId: item.modId,
      name: item.name,
      fileId: item.latestFileId,
      replaceFolder: item.folderName,
    });

    if (installed) await check();
  };

  return (
    <Panel
      eyebrow="GameBanana link"
      title={
        isChecking
          ? "Checking for updates…"
          : updates.length > 0
            ? `${updates.length} update${updates.length === 1 ? "" : "s"} available`
            : "Mods are up to date"
      }
      description={
        result
          ? `${result.checked} linked mod${result.checked === 1 ? "" : "s"} compared against GameBanana's latest files.`
          : "Comparing installed file IDs with GameBanana."
      }
      action={
        <Button
          size="sm"
          variant="outline"
          loading={isChecking}
          onClick={() => void check()}
        >
          <RefreshCw className="h-3.5 w-3.5" aria-hidden />
          Check again
        </Button>
      }
    >
      {error ? (
        <p className="text-xs leading-relaxed text-hot-400">{error}</p>
      ) : null}

      {isChecking && !result ? (
        <p className="flex items-center gap-2 text-xs text-zinc-500">
          <Loader2 className="h-3.5 w-3.5 animate-spin text-acid-400" aria-hidden />
          Reading GameBanana…
        </p>
      ) : null}

      {updates.length === 0 && result && !error ? (
        <p className="flex items-center gap-2 text-xs text-zinc-500">
          <CheckCircle2 className="h-3.5 w-3.5 text-acid-400" aria-hidden />
          {result.checked === 0
            ? "No mods are linked to GameBanana yet. Install one from Browse to enable update checks."
            : "Every linked mod already has the latest published file."}
        </p>
      ) : null}

      {updates.length > 0 ? (
        <ul className="space-y-2">
          {updates.map((item) => {
            const job = jobs[item.modId];
            const busyHere = Boolean(job);

            return (
              <li
                key={item.folderName}
                className="flex flex-wrap items-center justify-between gap-3 border border-ink-700/80 bg-ink-850/60 px-3 py-2"
              >
                <div className="min-w-0">
                  <p className="truncate font-display text-xs font-semibold text-zinc-100">
                    {item.name}
                  </p>
                  <p className="mt-0.5 truncate font-mono text-[10px] text-zinc-500">
                    {item.folderName}
                  </p>
                  <p className="mt-0.5 font-mono text-[10px] text-zinc-600">
                    <span className="text-zinc-400">
                      {item.installedVersion ?? "installed"}
                    </span>
                    {" → "}
                    <span className="text-acid-300">
                      {item.latestVersion ?? "latest file"}
                    </span>
                  </p>
                </div>

                <div className="flex items-center gap-2">
                  <Button
                    size="sm"
                    variant="primary"
                    loading={busyHere}
                    disabled={busyHere || item.latestFileId === null}
                    onClick={() => void updateOne(item)}
                  >
                    <ArrowDownToLine className="h-3.5 w-3.5" aria-hidden />
                    Update
                  </Button>
                  {busyHere ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => void cancel(item.modId)}
                      disabled={job?.cancelling ?? false}
                    >
                      <X className="h-3.5 w-3.5" aria-hidden />
{job?.cancelling ? "Cancelling..." : "Cancel"}
                    </Button>
                  ) : null}
                </div>
              </li>
            );
          })}
        </ul>
      ) : null}

      {failures.length > 0 ? (
        <p
          className={cn(
            "mt-3 font-mono text-[10px] leading-relaxed text-zinc-600",
            updates.length === 0 && "mt-2",
          )}
        >
          {failures.length} mod{failures.length === 1 ? "" : "s"} could not be
          checked ({failures[0]?.error ?? "unknown error"}).
        </p>
      ) : null}
    </Panel>
  );
}
