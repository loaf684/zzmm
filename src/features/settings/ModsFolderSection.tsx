import { AlertTriangle, FolderOpen, FolderSearch, Loader2, Radar, Save } from "lucide-react";

import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Panel } from "@/components/ui/Panel";
import { cn } from "@/lib/cn";
import type { ModsDirState } from "@/types/settings";

interface ModsFolderSectionProps {
  draft: string | null;
  saved: string | null;
  dirty: boolean;
  state: ModsDirState | null;
  validating: boolean;
  saving: boolean;
  detecting: boolean;
  onDraftChange: (value: string) => void;
  onBrowse: () => void;
  onSave: () => void;
  onDetect: () => void;
}

export function ModsFolderSection({
  draft,
  saved,
  dirty,
  state,
  validating,
  saving,
  detecting,
  onDraftChange,
  onBrowse,
  onSave,
  onDetect,
}: ModsFolderSectionProps) {
  const valid = Boolean(draft) && (state?.valid ?? false);

  return (
    <Panel
      eyebrow="Primary link"
      title="Mods directory"
      description="The folder ZZMI loads mods from. ZZMM reads and writes the same folder; nothing is moved or renamed until you explicitly toggle a mod."
      action={
        !draft ? (
          <Badge tone="muted">Not configured</Badge>
        ) : validating ? (
          <Badge tone="cyan">Checking…</Badge>
        ) : valid ? (
          <Badge tone="acid">Folder linked</Badge>
        ) : (
          <Badge tone="hot">Folder unavailable</Badge>
        )
      }
    >
      <label
        htmlFor="mods-dir"
        className="mb-2 block font-display text-[10px] uppercase tracking-[0.22em] text-zinc-500"
      >
        Folder path
      </label>

      <div className="flex flex-col gap-2 sm:flex-row">
        <div className="relative min-w-0 flex-1">
          <FolderOpen
            aria-hidden
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-600"
          />
          <input
            id="mods-dir"
            value={draft ?? ""}
            onChange={(event) => onDraftChange(event.target.value)}
            placeholder="C:\XXMI\ZZMI\Mods"
            spellCheck={false}
            autoComplete="off"
            aria-invalid={Boolean(draft) && Boolean(state) && !state?.valid}
            aria-describedby="mods-dir-status"
            className={cn(
              "h-10 w-full border bg-ink-950/80 pl-9 pr-3 font-mono text-xs text-zinc-200 placeholder:text-zinc-700 focus:outline-none",
              valid || !draft
                ? "border-ink-600 focus:border-acid-400/60"
                : "border-hot-500/50 focus:border-hot-400",
            )}
          />
        </div>

        <div className="flex gap-2">
          <Button variant="outline" onClick={onBrowse} className="flex-1 sm:flex-none">
            <FolderSearch className="h-3.5 w-3.5" aria-hidden />
            Browse
          </Button>
          <Button
            variant="primary"
            onClick={onSave}
            disabled={!dirty || !draft}
            loading={saving}
            className="flex-1 sm:flex-none"
          >
            <Save className="h-3.5 w-3.5" aria-hidden />
            Save
          </Button>
        </div>
      </div>

      <div id="mods-dir-status" className="mt-2 min-h-[18px]">
        {validating ? (
          <p className="flex items-center gap-2 text-[11px] text-cyan-300">
            <Loader2 className="h-3 w-3 animate-spin" aria-hidden />
            Checking folder…
          </p>
        ) : state && !state.valid ? (
          <p className="flex items-start gap-2 text-[11px] text-hot-400">
            <AlertTriangle className="mt-px h-3 w-3 shrink-0" aria-hidden />
            <span>{state.message ?? "This folder cannot be used."}</span>
          </p>
        ) : state?.valid ? (
          <p className="font-mono text-[11px] text-zinc-500">
            {state.modCount} {state.modCount === 1 ? "entry" : "entries"}
            <span className="px-1.5 text-zinc-700">·</span>
            {state.disabledCount} disabled
            {state.modCount === 0 ? (
              <>
                <span className="px-1.5 text-zinc-700">·</span>
                folder is empty
              </>
            ) : null}
          </p>
        ) : null}
      </div>

      <div className="mt-4 flex flex-wrap items-center gap-2">
        <Button variant="ghost" onClick={onDetect} loading={detecting}>
          <Radar className="h-3.5 w-3.5" aria-hidden />
          Detect installs
        </Button>

        {dirty ? (
          <span className="font-display text-[9px] uppercase tracking-[0.18em] text-acid-400/80">
            Unsaved changes
          </span>
        ) : saved ? (
          <span className="font-display text-[9px] uppercase tracking-[0.18em] text-zinc-600">
            Saved configuration
          </span>
        ) : null}
      </div>

      <p className="mt-4 border-t border-ink-800 pt-3 text-[11px] leading-relaxed text-zinc-600">
        Expected layout:{" "}
        <span className="font-mono text-zinc-400">...\ZZMI\Mods</span>, where each
        mod is its own folder containing one or more{" "}
        <span className="font-mono text-zinc-400">.ini</span> files. Disabled mods
        are prefixed with{" "}
        <span className="font-mono text-zinc-400">DISABLED_</span>.
      </p>
    </Panel>
  );
}
