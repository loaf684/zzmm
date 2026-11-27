import {
  CheckCircle2,
  Loader2,
  RefreshCw,
  RotateCcw,
  Wrench,
} from "lucide-react";

import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Notice } from "@/components/ui/Notice";
import { SectionHeading } from "@/components/ui/SectionHeading";
import { useFixerStore } from "@/store/fixer";

interface HashFixPanelProps {
  folderName: string;
}

/**
 * Repairs `hash = ...` values that a game update made stale.
 *
 * The check is lazy: a scan reads every INI in the mod, so it runs when asked
 * instead of on panel open. Fixing never deletes anything — each changed INI
 * keeps its original as `<file>.ini.bak`, which this panel can restore.
 */
export function HashFixPanel({ folderName }: HashFixPanelProps) {
  const entry = useFixerStore((state) => state.entries[folderName]);
  const scan = useFixerStore((state) => state.scan);
  const apply = useFixerStore((state) => state.apply);
  const restore = useFixerStore((state) => state.restore);

  const busy = entry?.busy ?? null;
  const result = entry?.scan ?? null;
  const error = entry?.error ?? null;
  const lastApply = entry?.lastApply ?? null;
  const lastRestore = entry?.lastRestore ?? null;

  const edits = result?.edits ?? 0;
  const files = result?.files.length ?? 0;
  const backups = result?.backups.length ?? 0;
  const failures = result?.failures ?? [];

  return (
    <section aria-label="Outdated hashes">
      <SectionHeading
        title={
          <span className="flex items-center gap-2">
            Hash fixer
            {result ? (
              edits > 0 ? (
                <Badge tone="hot">{edits} outdated</Badge>
              ) : (
                <Badge tone="acid">Up to date</Badge>
              )
            ) : null}
          </span>
        }
        action={
          <Button
            size="sm"
            variant="outline"
            loading={busy === "scan"}
            disabled={busy !== null && busy !== "scan"}
            onClick={() => void scan(folderName)}
          >
            <RefreshCw className="h-3 w-3" aria-hidden />
            {result ? "Recheck" : "Check"}
          </Button>
        }
      />

      <p className="mt-2 text-[11px] leading-relaxed text-zinc-500">
        Game updates renumber 3DMigoto hashes, which silently disables affected
        parts of older mods. Checking compares this mod's{" "}
        <span className="font-mono text-[10px] text-zinc-400">hash =</span>{" "}
        values against the table for game{" "}
        {result ? result.table.targetGameVersion : "the current release"}.
      </p>

      {busy === "scan" && !result ? (
        <div className="mt-2 flex items-center gap-2 text-[11px] text-zinc-500">
          <Loader2 className="h-3.5 w-3.5 animate-spin text-acid-400" aria-hidden />
          Reading INI files...
        </div>
      ) : null}

      {error ? (
        <Notice
          className="mt-2"
          message={error}
          action={
            <Button
              size="sm"
              variant="outline"
              onClick={() => void scan(folderName)}
            >
              Try again
            </Button>
          }
        />
      ) : null}

      {failures.length > 0 ? (
        <Notice
          className="mt-2"
          tone="warning"
          title={`${failures.length} file${failures.length === 1 ? "" : "s"} could not be read`}
          message={
            <ul className="space-y-0.5">
              {failures.map((failure) => (
                <li key={failure.relativePath}>
                  <span className="font-mono text-[10px]">
                    {failure.relativePath}
                  </span>
                  {" — "}
                  {failure.message}
                </li>
              ))}
            </ul>
          }
        />
      ) : null}

      {result && edits === 0 ? (
        <div className="mt-2 rounded-xl border border-acid-400/20 bg-acid-400/[0.04] px-3.5 py-3">
          <p className="flex items-center gap-1.5 text-[11px] font-medium text-acid-300">
            <CheckCircle2 className="h-3.5 w-3.5" aria-hidden />
            No outdated hashes found
          </p>
          <p className="mt-1 text-[11px] leading-relaxed text-zinc-500">
            Every hash this mod writes still matches game{" "}
            {result.table.targetGameVersion}. A broken part now comes from
            something else: a missing file, a nested folder, or a mod built for a
            different character.
          </p>
        </div>
      ) : null}

      {result && edits > 0 ? (
        <div className="mt-2 space-y-2">
          <div className="rounded-xl border border-hot-500/25 bg-hot-500/[0.04] px-3.5 py-3">
            <p className="flex items-center gap-1.5 text-[11px] font-medium text-hot-400">
              <Wrench className="h-3.5 w-3.5" aria-hidden />
              {edits} outdated hash value{edits === 1 ? "" : "s"} in {files} file
              {files === 1 ? "" : "s"}
            </p>
            <p className="mt-1 text-[11px] leading-relaxed text-zinc-500">
              Fixing writes the current values and keeps each original INI as{" "}
              <span className="font-mono text-[10px] text-zinc-400">
                .ini.bak
              </span>
              . Close the game first; afterwards press{" "}
              <span className="font-mono text-[10px] text-zinc-400">F10</span> in
              game to reload the mods.
            </p>
          </div>

          <div className="overflow-hidden rounded-2xl border border-ink-800/80 bg-ink-900/40">
            <div className="divide-y divide-ink-800/70">
              {result.files.map((file) => (
                <details key={file.relativePath} className="px-3.5 py-2">
                  <summary className="flex cursor-pointer list-none items-center justify-between gap-3 [&::-webkit-details-marker]:hidden">
                    <span
                      className="truncate font-mono text-[10px] text-zinc-400"
                      title={file.relativePath}
                    >
                      {file.relativePath}
                    </span>
                    <span className="font-display text-[9px] uppercase tracking-[0.18em] text-zinc-600">
                      {file.edits.length}
                    </span>
                  </summary>
                  <ul className="mt-1.5 space-y-1 pb-1">
                    {file.edits.map((edit) => (
                      <li
                        key={`${file.relativePath}-${edit.line}`}
                        className="flex items-center gap-1.5 font-mono text-[10px]"
                      >
                        <span className="text-zinc-600">L{edit.line}</span>
                        <span className="text-zinc-400">{edit.from}</span>
                        <span className="text-zinc-600" aria-hidden>
                          &rarr;
                        </span>
                        <span className="text-acid-300">{edit.to}</span>
                      </li>
                    ))}
                  </ul>
                </details>
              ))}
            </div>
          </div>

          <Button
            size="sm"
            variant="primary"
            loading={busy === "apply"}
            disabled={busy !== null && busy !== "apply"}
            onClick={() => void apply(folderName)}
          >
            <Wrench className="h-3 w-3" aria-hidden />
            Fix {edits} hash{edits === 1 ? "" : "es"}
          </Button>
        </div>
      ) : null}

      {lastApply ? (
        lastApply.written.length > 0 ? (
          <Notice
            className="mt-2"
            tone="success"
            title={`Fixed ${lastApply.edits} hash value${lastApply.edits === 1 ? "" : "s"}`}
            message={`Rewrote ${lastApply.written.length} file${lastApply.written.length === 1 ? "" : "s"}: ${lastApply.written.join(", ")}. Press F10 in game to reload.`}
          />
        ) : (
          <Notice
            className="mt-2"
            tone="warning"
            title="Nothing was written"
            message="No file could be updated. Check that the Mods folder is writable and the game is closed."
          />
        )
      ) : null}

      {lastRestore ? (
        <Notice
          className="mt-2"
          tone="success"
          title={
            lastRestore.restored.length > 0
              ? `Restored ${lastRestore.restored.length} file${lastRestore.restored.length === 1 ? "" : "s"}`
              : "Nothing to restore"
          }
          message={
            lastRestore.restored.length > 0
              ? `Back to the original INI content. Press F10 in game to reload.`
              : "Every INI already matches its backup."
          }
        />
      ) : null}

      {result && backups > 0 ? (
        <div className="mt-2 flex flex-wrap items-center justify-between gap-2 rounded-xl border border-ink-800/80 bg-ink-900/40 px-3.5 py-2.5">
          <p className="text-[11px] leading-relaxed text-zinc-500">
            {backups} INI file{backups === 1 ? " has" : "s have"} an{" "}
            <span className="font-mono text-[10px] text-zinc-400">.ini.bak</span>{" "}
            from ZZMM's first write.
          </p>
          <Button
            size="sm"
            variant="ghost"
            loading={busy === "restore"}
            disabled={busy !== null && busy !== "restore"}
            onClick={() => void restore(folderName)}
          >
            <RotateCcw className="h-3 w-3" aria-hidden />
            Restore backup
          </Button>
        </div>
      ) : null}

      {result ? (
        <p className="mt-2 text-[10px] leading-relaxed text-zinc-600">
          Table: game {result.table.targetGameVersion} · {result.table.renames}{" "}
          hash pairs · {result.table.source} data, generated {result.table.generated}.
        </p>
      ) : null}
    </section>
  );
}