import { useCallback, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowRight,
  CheckCircle2,
  FileWarning,
  Loader2,
  RotateCcw,
  ShieldCheck,
  Wrench,
} from "lucide-react";

import { Button } from "@/components/ui/Button";
import { Notice } from "@/components/ui/Notice";
import { Panel } from "@/components/ui/Panel";
import { api } from "@/lib/api";
import { cn } from "@/lib/cn";
import { useSettingsStore } from "@/store/settings";
import { toast } from "@/store/toasts";
import type {
  FixerApplyResult,
  FixerFile,
  FixerScanResult,
} from "@/types/fixer";

function shortPath(iniPath: string, modsDir: string): string {
  const relative = iniPath.startsWith(modsDir)
    ? iniPath.slice(modsDir.length).replace(/^[\\/]+/, "")
    : iniPath;
  return relative;
}

function changeSummary(file: FixerFile): string {
  const parts: string[] = [];
  if (file.updated.length > 0) {
    parts.push(
      `${file.updated.length} hash${file.updated.length === 1 ? "" : "es"} ${
        file.updated.length === 1 ? "is" : "are"
      } outdated`,
    );
  }
  if (file.additions.length > 0) {
    parts.push(
      `${file.additions.length} section${file.additions.length === 1 ? "" : "s"} missing`,
    );
  }
  return parts.join(" · ");
}

export function FixerPage() {
  const modsDir = useSettingsStore((state) => state.view?.settings.modsDir ?? "");

  const [scan, setScan] = useState<FixerScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [backup, setBackup] = useState(true);
  const [applying, setApplying] = useState(false);
  const [applied, setApplied] = useState<FixerApplyResult | null>(null);
  const [rollingBack, setRollingBack] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);

  const runScan = useCallback(async () => {
    setScanning(true);
    setError(null);
    setApplied(null);
    try {
      const result = await api.fixer.scan(modsDir);
      setScan(result);
      setSelected(new Set(result.files.map((file) => file.iniPath)));
      setExpanded(null);
      if (result.files.length === 0) {
        toast.success("Nothing to fix", `${result.scanned} ini files already match.`);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setScanning(false);
    }
  }, [modsDir]);

  const runApply = useCallback(async () => {
    const targets = [...selected];
    if (targets.length === 0) return;
    setApplying(true);
    setError(null);
    try {
      const result = await api.fixer.apply(modsDir, targets, backup);
      setApplied(result);
      const remaining = new Set(selected);
      for (const entry of result.applied) remaining.delete(entry.iniPath);
      setSelected(remaining);
      if (result.applied.length > 0) {
        toast.success(
          `Fixed ${result.applied.length} ini file${result.applied.length === 1 ? "" : "s"}`,
          backup ? "Backups were written before each change." : "No backups were kept.",
        );
      }
      if (result.failed.length > 0) {
        toast.error(
          `${result.failed.length} file${result.failed.length === 1 ? "" : "s"} failed`,
          result.failed[0]?.error,
        );
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setApplying(false);
    }
  }, [backup, modsDir, selected]);

  const runRollback = useCallback(async () => {
    const paths = applied?.applied.map((entry) => entry.iniPath) ?? [];
    if (paths.length === 0) return;
    setRollingBack(true);
    setError(null);
    try {
      const result = await api.fixer.rollback(modsDir, paths);
      if (result.restored.length > 0) {
        toast.success(
          `Restored ${result.restored.length} file${result.restored.length === 1 ? "" : "s"}`,
          "The original inis are back in place.",
        );
      }
      if (result.failed.length > 0) {
        toast.error("Some files could not be restored", result.failed[0]?.error);
      }
      setApplied(null);
      setScan(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setRollingBack(false);
    }
  }, [applied, modsDir]);

  const files = scan?.files ?? [];
  const totals = useMemo(() => {
    let hashes = 0;
    let sections = 0;
    let bufferPass = 0;
    for (const file of files) {
      hashes += file.updated.length;
      sections += file.additions.length;
      if (file.needsBufferPass) bufferPass += 1;
    }
    return { hashes, sections, bufferPass, unsupported: files.reduce((sum, file) => sum + file.unsupported.length, 0) };
  }, [files]);

  return (
    <div className="mx-auto w-full max-w-[1100px] px-8 py-8">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="eyebrow">Maintenance // Mods</p>
          <h1 className="mt-1.5 flex items-center gap-2 font-display text-[26px] font-semibold tracking-tight text-zinc-50">
            <Wrench className="h-5 w-5 text-acid-400" aria-hidden />
            Mod Fixer
          </h1>
          <p className="mt-1.5 max-w-2xl text-[13px] leading-relaxed text-zinc-500">
            Game updates change the hashes mods match on, which is why an outfit
            can stop loading after a patch. Scanning compares every ini in{" "}
            <span className="font-mono text-[11px] text-zinc-400">
              {modsDir || "your Mods folder"}
            </span>{" "}
            against the fixer database and shows exactly what it would rewrite.
            Nothing is written until you press fix.
          </p>
        </div>
        <Button onClick={() => void runScan()} loading={scanning} disabled={!modsDir}>
          <ShieldCheck className="h-3.5 w-3.5" aria-hidden />
          Scan mods folder
        </Button>
      </header>

      {error ? (
        <Notice
          className="mt-6"
          title="The fixer could not finish"
          message={error}
          onDismiss={() => setError(null)}
        />
      ) : null}

      {scan ? (
        <div className="mt-6 flex flex-wrap items-center gap-3 font-mono text-[10px] uppercase tracking-[0.16em] text-zinc-500">
          <span>{scan.scanned} ini files scanned</span>
          <span className="text-zinc-700">|</span>
          <span>{scan.skipped} skipped</span>
          <span className="text-zinc-700">|</span>
          <span className={files.length > 0 ? "text-acid-300" : "text-zinc-500"}>
            {files.length} need fixes
          </span>
          {totals.hashes > 0 ? (
            <>
              <span className="text-zinc-700">|</span>
              <span>{totals.hashes} hashes</span>
            </>
          ) : null}
          {totals.sections > 0 ? (
            <>
              <span className="text-zinc-700">|</span>
              <span>{totals.sections} sections</span>
            </>
          ) : null}
        </div>
      ) : null}

      {scan && totals.bufferPass > 0 ? (
        <Notice
          className="mt-4"
          tone="warning"
          title="Some mods need more than a hash rewrite"
          message={
            totals.bufferPass === 1
              ? "1 file also wants a buffer-level change that this version does not apply. It is fixed for hashes and sections; re-check that mod in game afterwards."
              : `${totals.bufferPass} files also want a buffer-level change that this version does not apply. They are fixed for hashes and sections; re-check those mods in game afterwards.`
          }
        />
      ) : null}

      {files.length > 0 ? (
        <Panel
          className="mt-6"
          eyebrow="Scan result"
          title={`${selected.size} of ${files.length} selected`}
          action={
            <div className="flex items-center gap-2">
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  setSelected(
                    selected.size === files.length
                      ? new Set()
                      : new Set(files.map((file) => file.iniPath)),
                  )
                }
              >
                {selected.size === files.length ? "Select none" : "Select all"}
              </Button>
              <Button
                variant="primary"
                size="sm"
                loading={applying}
                disabled={selected.size === 0}
                onClick={() => void runApply()}
              >
                <Wrench className="h-3.5 w-3.5" aria-hidden />
                Fix {selected.size} mod{selected.size === 1 ? "" : "s"}
              </Button>
            </div>
          }
        >
          <label className="flex items-center gap-2 border-b border-ink-700/60 px-5 py-3 text-[12px] text-zinc-400">
            <input
              type="checkbox"
              checked={backup}
              onChange={(event) => setBackup(event.target.checked)}
              className="h-3.5 w-3.5 accent-acid-400"
            />
            Keep a backup of every changed ini (written next to the mod, the same
            way the community fixer does)
          </label>

          <ul className="divide-y divide-ink-700/60">
            {files.map((file) => {
              const isSelected = selected.has(file.iniPath);
              const isOpen = expanded === file.iniPath;
              return (
                <li key={file.iniPath}>
                  <div className="flex items-start gap-3 px-5 py-3">
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={() =>
                        setSelected((current) => {
                          const next = new Set(current);
                          if (next.has(file.iniPath)) next.delete(file.iniPath);
                          else next.add(file.iniPath);
                          return next;
                        })
                      }
                      aria-label={`Fix ${file.folder}`}
                      className="mt-0.5 h-3.5 w-3.5 accent-acid-400"
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-display text-[13px] font-semibold text-zinc-100">
                          {file.folder}
                        </span>
                        {file.character ? (
                          <span className="border border-ink-700 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-[0.14em] text-zinc-400">
                            {file.character}
                          </span>
                        ) : null}
                        {file.needsBufferPass ? (
                          <span className="inline-flex items-center gap-1 border border-hot-500/40 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-[0.14em] text-hot-400">
                            <AlertTriangle className="h-3 w-3" aria-hidden />
                            buffer pass
                          </span>
                        ) : null}
                      </div>
                      <p className="mt-1 truncate font-mono text-[10px] text-zinc-600" title={file.iniPath}>
                        {shortPath(file.iniPath, modsDir)}
                      </p>
                      <p className="mt-1 text-[11px] text-zinc-500">{changeSummary(file)}</p>
                    </div>
                    <button
                      type="button"
                      onClick={() => setExpanded(isOpen ? null : file.iniPath)}
                      aria-expanded={isOpen}
                      className="shrink-0 font-mono text-[9px] uppercase tracking-[0.16em] text-zinc-500 transition-colors hover:text-acid-300"
                    >
                      {isOpen ? "hide" : "details"}
                    </button>
                  </div>

                  {isOpen ? (
                    <div className="border-t border-ink-700/40 bg-ink-950/40 px-5 py-3">
                      <ul className="space-y-1.5">
                        {file.updated.map((change) => (
                          <li
                            key={`${change.to}-${change.from ?? "new"}`}
                            className="flex flex-wrap items-center gap-2 font-mono text-[10px] text-zinc-500"
                          >
                            <span className="text-zinc-600">
                              {change.from ?? "(missing)"}
                            </span>
                            <ArrowRight className="h-3 w-3 text-zinc-700" aria-hidden />
                            <span className="text-acid-300">{change.to}</span>
                            <span className="truncate text-zinc-500">{change.label}</span>
                          </li>
                        ))}
                        {file.additions.map((addition) => (
                          <li
                            key={`${addition.hash}-${addition.section}`}
                            className="flex flex-wrap items-center gap-2 font-mono text-[10px] text-zinc-500"
                          >
                            <CheckCircle2 className="h-3 w-3 text-acid-400" aria-hidden />
                            <span className="text-zinc-400">add section</span>
                            <span className="text-zinc-300">{addition.section}</span>
                            <span className="text-zinc-600">for {addition.hash}</span>
                          </li>
                        ))}
                        {file.unsupported.map((entry) => (
                          <li
                            key={`${entry.hash}-${entry.command}`}
                            className="flex flex-wrap items-center gap-2 font-mono text-[10px] text-hot-300"
                          >
                            <FileWarning className="h-3 w-3" aria-hidden />
                            {entry.command}
                            <span className="text-zinc-500">{entry.label}</span>
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        </Panel>
      ) : null}

      {scan && files.length === 0 ? (
        <Panel className="mt-6" eyebrow="Scan result" title="Everything matches">
          <div className="flex flex-col items-center gap-3 py-8 text-center">
            <ShieldCheck className="h-8 w-8 text-acid-400" aria-hidden />
            <p className="max-w-md text-xs leading-relaxed text-zinc-500">
              No ini in {modsDir || "your Mods folder"} carries a hash or a
              missing IB section that this fixer knows about. Mods that still
              misbehave are usually a folder-layout problem rather than a hash
              one.
            </p>
          </div>
        </Panel>
      ) : null}

      {applied && applied.applied.length > 0 ? (
        <Panel
          className="mt-6"
          eyebrow="Last run"
          title={`${applied.applied.length} file${applied.applied.length === 1 ? "" : "s"} rewritten`}
          action={
            <Button variant="outline" size="sm" loading={rollingBack} onClick={() => void runRollback()}>
              <RotateCcw className="h-3.5 w-3.5" aria-hidden />
              Restore originals
            </Button>
          }
        >
          <ul className="divide-y divide-ink-700/60">
            {applied.applied.map((entry) => (
              <li key={entry.iniPath} className="px-5 py-3">
                <p className="font-mono text-[10px] text-zinc-400">
                  {shortPath(entry.iniPath, modsDir)}
                </p>
                <p className="mt-1 font-mono text-[10px] text-zinc-600">
                  {entry.updated.length} hash
                  {entry.updated.length === 1 ? "" : "es"} updated
                  {entry.additions.length > 0
                    ? ` · ${entry.additions.length} section${entry.additions.length === 1 ? "" : "s"} added`
                    : ""}
                  {entry.backupPath
                    ? ` · backup ${shortPath(entry.backupPath, modsDir)}`
                    : " · no backup"}
                </p>
              </li>
            ))}
          </ul>
        </Panel>
      ) : null}

      {applied && applied.failed.length > 0 ? (
        <Panel className="mt-4" eyebrow="Failures" title="These files were left alone">
          <ul className="divide-y divide-ink-700/60">
            {applied.failed.map((entry) => (
              <li key={entry.iniPath} className="px-5 py-3">
                <p className="font-mono text-[10px] text-hot-300">
                  {shortPath(entry.iniPath, modsDir)}
                </p>
                <p className="mt-1 text-[11px] text-zinc-500">{entry.error}</p>
              </li>
            ))}
          </ul>
        </Panel>
      ) : null}

      <p className="mt-6 text-[11px] leading-relaxed text-zinc-600">
        Hash database from{" "}
        <span className="font-mono text-[10px] text-zinc-500">
          Vonksdesu/ZZZ-Mod-Fixer
        </span>{" "}
        (community continuation of petrascyll&apos;s tool). Fixing changes files
        inside your Mods folder - keep the backup option on unless you already
        keep your own copies.
      </p>

      {scanning ? (
        <div className="mt-4 flex items-center gap-2 font-display text-[10px] uppercase tracking-[0.2em] text-zinc-500">
          <Loader2 className="h-3.5 w-3.5 animate-spin text-acid-400" aria-hidden />
          Reading every ini in the mods folder...
        </div>
      ) : null}
    </div>
  );
}

export { cn };