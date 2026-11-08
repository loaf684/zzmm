import { useEffect, useState } from "react";
import {
  ArchiveRestore,
  Loader2,
  Save,
  Trash2,
} from "lucide-react";

import { Button } from "@/components/ui/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { IconButton } from "@/components/ui/IconButton";
import { Panel } from "@/components/ui/Panel";
import { cn } from "@/lib/cn";
import { formatIsoDate } from "@/lib/format";
import { useLoadoutsStore } from "@/store/loadouts";

interface LoadoutsPanelProps {
  /** False while no Mods folder is linked; the panel stays hidden. */
  enabled: boolean;
}

export function LoadoutsPanel({ enabled }: LoadoutsPanelProps) {
  const loadouts = useLoadoutsStore((state) => state.loadouts);
  const isLoading = useLoadoutsStore((state) => state.isLoading);
  const isSaving = useLoadoutsStore((state) => state.isSaving);
  const busyName = useLoadoutsStore((state) => state.busyName);
  const error = useLoadoutsStore((state) => state.error);
  const load = useLoadoutsStore((state) => state.load);
  const save = useLoadoutsStore((state) => state.save);
  const remove = useLoadoutsStore((state) => state.remove);
  const apply = useLoadoutsStore((state) => state.apply);

  const [name, setName] = useState("");
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);

  useEffect(() => {
    if (enabled) void load();
  }, [enabled, load]);

  if (!enabled) return null;

  const saveCurrent = async () => {
    const trimmed = name.trim();
    if (!trimmed) return;

    if (await save(trimmed)) setName("");
  };

  return (
    <Panel
      eyebrow="Loadouts"
      title="Saved mod states"
      description="Snapshot which mods are enabled, then switch back to that setup at any time. Applying a loadout only touches mods that still exist and writes a backup first."
    >
      <div className="flex flex-wrap items-center gap-2">
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void saveCurrent();
            }
          }}
          maxLength={80}
          placeholder="Loadout name, e.g. Daily"
          aria-label="Loadout name"
          className="h-9 min-w-[200px] flex-1 rounded-lg border border-ink-700 bg-ink-900 px-3 text-[12px] text-zinc-200 placeholder:text-zinc-600 transition-colors focus:border-ink-500 focus:outline-none"
        />
        <Button
          variant="primary"
          onClick={() => void saveCurrent()}
          loading={isSaving}
          disabled={!name.trim()}
        >
          <Save className="h-3.5 w-3.5" aria-hidden />
          Save current
        </Button>
      </div>

      {error ? (
        <p className="mt-3 text-[11px] text-hot-400">{error}</p>
      ) : null}

      <div className="mt-4 space-y-2">
        {isLoading && loadouts.length === 0 ? (
          <div className="flex items-center gap-2 py-2 text-[11px] text-zinc-500">
            <Loader2 className="h-3.5 w-3.5 animate-spin text-acid-400" aria-hidden />
            Reading loadouts…
          </div>
        ) : loadouts.length === 0 ? (
          <p className="py-1 text-[11px] leading-relaxed text-zinc-600">
            No loadouts yet. Save the current setup to switch between it and
            other configurations later.
          </p>
        ) : (
          loadouts.map((loadout) => {
            const busy = busyName === loadout.name;

            return (
              <div
                key={loadout.name}
                className={cn(
                  "flex flex-wrap items-center gap-3 rounded-lg border border-ink-700/70 bg-ink-900/60 px-3 py-2",
                  busy && "opacity-70",
                )}
              >
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[12px] font-medium text-zinc-200">
                    {loadout.name}
                  </p>
                  <p className="mt-0.5 text-[10px] text-zinc-500">
                    {loadout.entryCount} mod{loadout.entryCount === 1 ? "" : "s"}{" "}
                    · {loadout.enabledCount} enabled · updated{" "}
                    {formatIsoDate(loadout.updatedAt)}
                  </p>
                </div>

                <div className="flex items-center gap-1.5">
                  <Button
                    variant="outline"
                    size="sm"
                    loading={busy}
                    disabled={busyName !== null}
                    onClick={() => void apply(loadout.name)}
                  >
                    <ArchiveRestore className="h-3.5 w-3.5" aria-hidden />
                    Apply
                  </Button>
                  <IconButton
                    label={`Delete ${loadout.name}`}
                    tone="danger"
                    disabled={busyName !== null}
                    onClick={() => setPendingDelete(loadout.name)}
                  >
                    <Trash2 className="h-3.5 w-3.5" aria-hidden />
                  </IconButton>
                </div>
              </div>
            );
          })
        )}
      </div>

      <ConfirmDialog
        open={pendingDelete !== null}
        title="Delete this loadout?"
        description={
          pendingDelete
            ? `${pendingDelete} will be removed. Your mods keep their current enabled state.`
            : ""
        }
        confirmLabel="Delete loadout"
        destructive
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          const target = pendingDelete;
          setPendingDelete(null);
          if (target) void remove(target);
        }}
      />
    </Panel>
  );
}
