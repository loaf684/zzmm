import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArchiveRestore,
  ArrowRight,
  Ban,
  CheckCircle2,
  FileWarning,
  History,
  Loader2,
  RotateCcw,
  ShieldCheck,
  Undo2,
  Wrench,
} from "lucide-react";

import { Button } from "@/components/ui/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { Notice } from "@/components/ui/Notice";
import { Panel } from "@/components/ui/Panel";
import { api } from "@/lib/api";
import type { FixerIgnoreTarget } from "@/lib/api/fixer";
import { cn } from "@/lib/cn";
import { formatDate } from "@/lib/format";
import { useFixerStore } from "@/store/fixer";
import { useSettingsStore } from "@/store/settings";
import { toast } from "@/store/toasts";
import type {
  FixerApplyResult,
  FixerFile,
  FixerHistoryEntry,
  FixerIgnoredEntry,
  FixerScanResult,
  IgnoreScope,
} from "@/types/fixer";

function shortPath(iniPath: string, modsDir: string): string {
  const relative = iniPath.startsWith(modsDir)
    ? iniPath.slice(modsDir.length).replace(/^[\\/]+/, "")
    : iniPath;
  return relative;
}

/** Windows paths: case and separators do not tell two files apart. */
function normalize(value: string): string {
  return value.replace(/\//g, "\\").toLowerCase();
}

/** Which whole-file ignore entry covers this ini, most specific first. */
function fileScopeOf(
  entries: FixerIgnoredEntry[],
  folder: string,
  iniPath: string,
): IgnoreScope | null {
  const ini = normalize(iniPath);
  const mod = normalize(folder);

  if (
    entries.some(
      (entry) =>
        entry.iniPath !== null &&
        entry.key === null &&
        normalize(entry.iniPath) === ini,
    )
  ) {
    return "file";
  }

  if (
    entries.some(
      (entry) => entry.iniPath === null && normalize(entry.folder) === mod,
    )
  ) {
    return "mod";
  }

  return null;
}

function ignoredKeysOf(
  entries: FixerIgnoredEntry[],
  iniPath: string,
): Set<string> {
  const ini = normalize(iniPath);
  const keys = new Set<string>();

  for (const entry of entries) {
    if (
      entry.iniPath !== null &&
      entry.key !== null &&
      normalize(entry.iniPath) === ini
    ) {
      keys.add(entry.key);
    }
  }

  return keys;
}

/**
 * The flags `fixer_scan` sets, recomputed from the ignore list the backend just
 * returned, so a toggle updates the list without another full scan. The backend
 * stays the source of truth: it refuses to write anything this marks ignored.
 */
function withIgnoreFlags(
  file: FixerFile,
  entries: FixerIgnoredEntry[],
): FixerFile {
  const scope = fileScopeOf(entries, file.folder, file.iniPath);
  const keys = ignoredKeysOf(entries, file.iniPath);

  const updated = file.updated.map((change) => ({
    ...change,
    ignored: scope !== null || keys.has(change.key),
  }));
  const additions = file.additions.map((addition) => ({
    ...addition,
    ignored: scope !== null || keys.has(addition.key),
  }));

  const hasRows = updated.length + additions.length > 0;
  const everyRowIgnored =
    hasRows &&
    updated.every((change) => change.ignored) &&
    additions.every((addition) => addition.ignored);
  const ignoredScope = scope ?? (everyRowIgnored ? "fix" : null);

  return {
    ...file,
    updated,
    additions,
    ignored: ignoredScope !== null,
    ignoredScope,
  };
}

/** A file an apply would still touch: one recommendation is not ignored. */
function isActionable(file: FixerFile): boolean {
  return (
    file.updated.some((change) => !change.ignored) ||
    file.additions.some((addition) => !addition.ignored)
  );
}

const SCOPE_LABEL: Record<IgnoreScope, string> = {
  mod: "whole mod",
  file: "ini file",
  fix: "single fix",
};

function changeSummary(file: FixerFile): string {
  const parts: string[] = [];
  const updates = file.updated.filter((change) => !change.ignored).length;
  const additions = file.additions.filter((addition) => !addition.ignored).length;

  if (updates > 0) {
    parts.push(
      `${updates} hash${updates === 1 ? "" : "es"} ${
        updates === 1 ? "is" : "are"
      } outdated`,
    );
  }
  if (additions > 0) {
    parts.push(
      `${additions} section${additions === 1 ? "" : "s"} missing`,
    );
  }

  const ignoredCount =
    file.updated.length - updates + (file.additions.length - additions);
  if (ignoredCount > 0) {
    parts.push(`${ignoredCount} ignored`);
  }

  return parts.join(" · ");
}

/** Accurate answer to "can this fix be undone?", from the backend's disk check. */
function restoreStatus(entry: FixerHistoryEntry): string {
  if (entry.restoredAt) return `restored ${formatDate(entry.restoredAt)}`;
  if (!entry.backupPath) return "no backup was kept";
  if (!entry.backupAvailable) return "backup no longer on disk";
  return "backup ready";
}

export function FixerPage() {
  const modsDir = useSettingsStore((state) => state.view?.settings.modsDir ?? "");
  const ignored = useFixerStore((state) => state.ignored);
  const history = useFixerStore((state) => state.history);
  const fixerStateError = useFixerStore((state) => state.loadError);
  const loadFixerState = useFixerStore((state) => state.load);
  const setIgnored = useFixerStore((state) => state.setIgnored);

  const [scan, setScan] = useState<FixerScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [backup, setBackup] = useState(true);
  const [applying, setApplying] = useState(false);
  const [applied, setApplied] = useState<FixerApplyResult | null>(null);
  const [rollingBack, setRollingBack] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [pendingRestore, setPendingRestore] = useState<{
    entries: FixerHistoryEntry[];
    title: string;
    description: string;
  } | null>(null);
  const [restoring, setRestoring] = useState(false);

  // The ignore list and the history live in the config folder, so this is what
  // makes both survive leaving the Fixer route and coming back.
  useEffect(() => {
    void loadFixerState();
  }, [loadFixerState]);

  const rowLabel = (file: FixerFile) =>
    `${file.folder} · ${shortPath(file.iniPath, modsDir)}`;

  /** Write one ignore entry and re-derive the scanned rows from the result. */
  const applyIgnore = useCallback(
    async (
      target: FixerIgnoreTarget,
      ignoredNow: boolean,
    ): Promise<boolean> => {
      const view = await setIgnored(target, ignoredNow);
      if (!view) return false;
      if (!scan) return true;

      const files = scan.files.map((file) => withIgnoreFlags(file, view.ignored));
      setScan({ ...scan, files });
      setSelected((current) => {
        const next = new Set(current);
        for (const file of files) {
          if (!isActionable(file)) next.delete(file.iniPath);
        }
        return next;
      });
      return true;
    },
    [scan, setIgnored],
  );

  const runScan = useCallback(async () => {
    setScanning(true);
    setError(null);
    setApplied(null);
    try {
      const result = await api.fixer.scan(modsDir);
      setScan(result);
      // Ignored files stay listed so they remain visible, but they are not
      // offered for fixing.
      setSelected(
        new Set(
          result.files
            .filter((file) => !file.ignored || isActionable(file))
            .map((file) => file.iniPath),
        ),
      );
      setExpanded(null);
      if (result.files.filter(isActionable).length === 0) {
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
      for (const ignoredPath of result.ignored) remaining.delete(ignoredPath);
      setSelected(remaining);
      if (result.applied.length > 0) {
        toast.success(
          `Fixed ${result.applied.length} ini file${result.applied.length === 1 ? "" : "s"}`,
          backup ? "Backups were written before each change." : "No backups were kept.",
        );
      }
      if (result.ignored.length > 0) {
        toast.info(
          `Left ${result.ignored.length} file${result.ignored.length === 1 ? "" : "s"} alone`,
          "They are on the ignore list. Unignore them on this page to fix them.",
        );
      }
      if (result.failed.length > 0) {
        toast.error(
          `${result.failed.length} file${result.failed.length === 1 ? "" : "s"} failed`,
          result.failed[0]?.error,
        );
      }
      // The files that were written just became history entries.
      await loadFixerState();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setApplying(false);
    }
  }, [backup, loadFixerState, modsDir, selected]);

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
      // Those backups are consumed now, so availability changed.
      await loadFixerState();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setRollingBack(false);
    }
  }, [applied, loadFixerState, modsDir]);

  const runRestore = useCallback(
    async (entries: FixerHistoryEntry[]) => {
      // A fix whose backup is gone is never offered as restorable, and the
      // backend refuses it anyway.
      const targets = entries.filter(
        (entry) => entry.backupAvailable && !entry.restoredAt,
      );
      if (targets.length === 0) {
        setPendingRestore(null);
        return;
      }

      setRestoring(true);
      setError(null);
      let restored = 0;
      const failures: string[] = [];

      for (const entry of targets) {
        try {
          const result = await api.fixer.restore(
            modsDir,
            entry.iniPath,
            entry.fixedAt,
          );
          restored += result.restored.length;
          for (const failure of result.failed) {
            failures.push(`${shortPath(failure.iniPath, modsDir)}: ${failure.error}`);
          }
        } catch (cause) {
          failures.push(
            `${shortPath(entry.iniPath, modsDir)}: ${
              cause instanceof Error ? cause.message : String(cause)
            }`,
          );
        }
      }

      // Backups were consumed and the entries were marked restored.
      await loadFixerState();
      if (restored > 0) {
        toast.success(
          `Restored ${restored} file${restored === 1 ? "" : "s"}`,
          "The original inis are back in place.",
        );
        // The files on disk no longer match the last scan.
        setScan(null);
        setApplied(null);
      }
      if (failures.length > 0) {
        toast.error("Some files could not be restored", failures[0]);
      }

      setRestoring(false);
      setPendingRestore(null);
    },
    [loadFixerState, modsDir],
  );

  const toggleRowIgnore = useCallback(
    async (file: FixerFile) => {
      if (!file.ignored) {
        await applyIgnore(
          { folder: file.folder, iniPath: file.iniPath, label: rowLabel(file) },
          true,
        );
        return;
      }

      if (file.ignoredScope === "mod") {
        await applyIgnore(
          { folder: file.folder, iniPath: null, label: file.folder },
          false,
        );
        return;
      }

      if (file.ignoredScope === "file") {
        await applyIgnore(
          { folder: file.folder, iniPath: file.iniPath, label: rowLabel(file) },
          false,
        );
        return;
      }

      // `fix`: the recommendations were ignored one by one, so removing them
      // one by one is what brings the file back.
      const ini = normalize(file.iniPath);
      const entries = ignored.filter(
        (entry) =>
          entry.iniPath !== null &&
          entry.key !== null &&
          normalize(entry.iniPath) === ini,
      );
      for (const entry of entries) {
        const ok = await applyIgnore(
          {
            folder: entry.folder,
            iniPath: entry.iniPath,
            key: entry.key,
            label: entry.label,
          },
          false,
        );
        if (!ok) return;
      }
    },
    [applyIgnore, ignored],
  );

  const toggleModIgnore = useCallback(
    async (file: FixerFile) => {
      const mod = normalize(file.folder);
      const modIgnored = ignored.some(
        (entry) => entry.iniPath === null && normalize(entry.folder) === mod,
      );
      await applyIgnore(
        { folder: file.folder, iniPath: null, label: file.folder },
        !modIgnored,
      );
    },
    [applyIgnore, ignored],
  );

  const runUnignore = useCallback(
    async (entry: FixerIgnoredEntry) => {
      await applyIgnore(
        {
          folder: entry.folder,
          iniPath: entry.iniPath,
          key: entry.key,
          label: entry.label,
        },
        false,
      );
    },
    [applyIgnore],
  );

  const files = scan?.files ?? [];
  const totals = useMemo(() => {
    let hashes = 0;
    let sections = 0;
    let bufferPass = 0;
    let ignoredFiles = 0;
    for (const file of files) {
      hashes += file.updated.filter((change) => !change.ignored).length;
      sections += file.additions.filter((addition) => !addition.ignored).length;
      if (file.needsBufferPass) bufferPass += 1;
      if (file.ignored) ignoredFiles += 1;
    }
    return {
      hashes,
      sections,
      bufferPass,
      ignoredFiles,
      unsupported: files.reduce((sum, file) => sum + file.unsupported.length, 0),
    };
  }, [files]);

  const runs = useMemo(() => {
    const grouped = new Map<number, FixerHistoryEntry[]>();
    for (const entry of history) {
      const list = grouped.get(entry.fixedAt);
      if (list) list.push(entry);
      else grouped.set(entry.fixedAt, [entry]);
    }
    return [...grouped.entries()]
      .map(([fixedAt, entries]) => ({ fixedAt, entries }))
      .sort((left, right) => right.fixedAt - left.fixedAt);
  }, [history]);

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

      {fixerStateError ? (
        <Notice
          className="mt-4"
          tone="warning"
          title="The ignore list could not be read"
          message={`${fixerStateError} Fixing still works, and the fixer keeps honouring what you ignored before.`}
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
          {totals.ignoredFiles > 0 ? (
            <>
              <span className="text-zinc-700">|</span>
              <span className="text-amber-300">{totals.ignoredFiles} ignored</span>
            </>
          ) : null}
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
                    selected.size === files.filter(isActionable).length
                      ? new Set()
                      : new Set(
                          files.filter(isActionable).map((file) => file.iniPath),
                        ),
                  )
                }
              >
                {selected.size === files.filter(isActionable).length
                  ? "Select none"
                  : "Select all"}
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
              const scopeIgnored =
                file.ignoredScope === "mod" || file.ignoredScope === "file";
              const mod = normalize(file.folder);
              const modIgnored = ignored.some(
                (entry) => entry.iniPath === null && normalize(entry.folder) === mod,
              );

              const ignoreButton = (
                key: string,
                ignoredNow: boolean,
                label: string,
                className: string,
              ) => (
                <button
                  type="button"
                  onClick={() =>
                    void applyIgnore(
                      {
                        folder: file.folder,
                        iniPath: file.iniPath,
                        key,
                        label,
                      },
                      !ignoredNow,
                    )
                  }
                  className={className}
                >
                  {ignoredNow ? "unignore" : "ignore"}
                </button>
              );

              return (
                <li key={file.iniPath} className={cn(file.ignored && "opacity-70")}>
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
                        {file.ignored ? (
                          <span className="inline-flex items-center gap-1 border border-amber-400/40 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-[0.14em] text-amber-300">
                            <Ban className="h-3 w-3" aria-hidden />
                            ignored ·{" "}
                            {SCOPE_LABEL[file.ignoredScope ?? "file"]}
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
                      onClick={() => void toggleRowIgnore(file)}
                      title={
                        file.ignored
                          ? "Let the fixer touch this ini again"
                          : "Never fix this ini: nothing is written to it"
                      }
                      className="shrink-0 font-mono text-[9px] uppercase tracking-[0.16em] text-amber-300/80 transition-colors hover:text-amber-200"
                    >
                      {file.ignored ? "unignore" : "ignore"}
                    </button>
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
                            key={change.key}
                            className={cn(
                              "flex flex-wrap items-center gap-2 font-mono text-[10px] text-zinc-500",
                              change.ignored && "opacity-60",
                            )}
                          >
                            <span
                              className={cn(
                                "text-zinc-600",
                                change.ignored && "line-through",
                              )}
                            >
                              {change.from ?? "(missing)"}
                            </span>
                            <ArrowRight className="h-3 w-3 text-zinc-700" aria-hidden />
                            <span className="text-acid-300">{change.to}</span>
                            <span className="truncate text-zinc-500">{change.label}</span>
                            {change.ignored ? (
                              <span className="font-mono text-[9px] uppercase tracking-[0.14em] text-amber-300/80">
                                ignored
                              </span>
                            ) : null}
                            {scopeIgnored
                              ? null
                              : ignoreButton(
                                  change.key,
                                  change.ignored,
                                  `${rowLabel(file)} · ${
                                    change.from ?? "(missing)"
                                  } → ${change.to}`,
                                  "shrink-0 font-mono text-[9px] uppercase tracking-[0.16em] text-zinc-500 transition-colors hover:text-amber-300",
                                )}
                          </li>
                        ))}
                        {file.additions.map((addition) => (
                          <li
                            key={addition.key}
                            className={cn(
                              "flex flex-wrap items-center gap-2 font-mono text-[10px] text-zinc-500",
                              addition.ignored && "opacity-60",
                            )}
                          >
                            <CheckCircle2 className="h-3 w-3 text-acid-400" aria-hidden />
                            <span className="text-zinc-400">add section</span>
                            <span
                              className={cn(
                                "text-zinc-300",
                                addition.ignored && "line-through",
                              )}
                            >
                              {addition.section}
                            </span>
                            <span className="text-zinc-600">for {addition.hash}</span>
                            {addition.ignored ? (
                              <span className="font-mono text-[9px] uppercase tracking-[0.14em] text-amber-300/80">
                                ignored
                              </span>
                            ) : null}
                            {scopeIgnored
                              ? null
                              : ignoreButton(
                                  addition.key,
                                  addition.ignored,
                                  `${rowLabel(file)} · add ${addition.section}`,
                                  "shrink-0 font-mono text-[9px] uppercase tracking-[0.16em] text-zinc-500 transition-colors hover:text-amber-300",
                                )}
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

                      <div className="mt-3 flex flex-wrap items-center justify-between gap-2 border-t border-ink-700/40 pt-3">
                        <p className="max-w-xl text-[11px] leading-relaxed text-zinc-600">
                          Ignoring writes nothing to the ini. It only tells the
                          fixer to skip this fix in every later scan and apply;
                          the list is kept in ZZMM&apos;s config folder.
                        </p>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => void toggleModIgnore(file)}
                        >
                          <Ban className="h-3.5 w-3.5" aria-hidden />
                          {modIgnored
                            ? "Unignore the whole mod"
                            : "Ignore the whole mod"}
                        </Button>
                      </div>
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

      {ignored.length > 0 ? (
        <Panel
          className="mt-6"
          eyebrow="Ignore list"
          title={`${ignored.length} fix${ignored.length === 1 ? "" : "es"} the fixer leaves alone`}
          description="Saved in ZZMM's config folder, so they survive restarts and later scans. The fixer never writes to an ignored ini, and nothing here edits a mod."
        >
          <ul className="divide-y divide-ink-700/60">
            {[...ignored]
              .sort((left, right) => right.ignoredAt - left.ignoredAt)
              .map((entry) => {
                const scope: IgnoreScope =
                  entry.key !== null
                    ? "fix"
                    : entry.iniPath !== null
                      ? "file"
                      : "mod";

                return (
                  <li
                    key={`${entry.folder}|${entry.iniPath ?? ""}|${entry.key ?? ""}`}
                    className="flex flex-wrap items-center justify-between gap-3 py-2.5"
                  >
                    <div className="min-w-0">
                      <p
                        className="truncate font-mono text-[10px] text-zinc-400"
                        title={entry.label}
                      >
                        {entry.label}
                      </p>
                      <p className="mt-0.5 font-mono text-[9px] uppercase tracking-[0.14em] text-zinc-600">
                        {SCOPE_LABEL[scope]} · since {formatDate(entry.ignoredAt)}
                      </p>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void runUnignore(entry)}
                    >
                      <Undo2 className="h-3.5 w-3.5" aria-hidden />
                      Unignore
                    </Button>
                  </li>
                );
              })}
          </ul>
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
                    ? ` · backup ${entry.backupPath}`
                    : " · no backup"}
                </p>
              </li>
            ))}
          </ul>
        </Panel>
      ) : null}

      {applied && applied.ignored.length > 0 ? (
        <Panel
          className="mt-4"
          eyebrow="Left alone"
          title={`${applied.ignored.length} file${applied.ignored.length === 1 ? "" : "s"} ignored`}
        >
          <ul className="divide-y divide-ink-700/60">
            {applied.ignored.map((iniPath) => (
              <li key={iniPath} className="px-5 py-3">
                <p className="font-mono text-[10px] text-zinc-500">
                  {shortPath(iniPath, modsDir)}
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

      {runs.length > 0 ? (
        <Panel
          className="mt-6"
          eyebrow="Fix history"
          title="Previous fixes"
          description="Every ini ZZMM rewrote, newest run first, kept in ZZMM's config folder. Restore puts the file back from the backup that run wrote; a fix whose backup is gone cannot be restored."
        >
          <ul className="space-y-4">
            {runs.map((run) => {
              const restorable = run.entries.filter(
                (entry) => entry.backupAvailable && !entry.restoredAt,
              );
              return (
                <li key={run.fixedAt} className="border border-ink-700/50">
                  <div className="flex flex-wrap items-center justify-between gap-2 border-b border-ink-700/50 px-4 py-2.5">
                    <p className="flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.16em] text-zinc-500">
                      <History className="h-3.5 w-3.5 text-zinc-600" aria-hidden />
                      {formatDate(run.fixedAt)} · {run.entries.length} file
                      {run.entries.length === 1 ? "" : "s"}
                      {restorable.length < run.entries.length
                        ? ` · ${run.entries.length - restorable.length} not restorable`
                        : ""}
                    </p>
                    {restorable.length > 0 ? (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() =>
                          setPendingRestore({
                            entries: run.entries,
                            title:
                              restorable.length === 1
                                ? "Restore this file?"
                                : `Restore ${restorable.length} files?`,
                            description:
                              restorable.length === 1
                                ? `${shortPath(restorable[0].iniPath, modsDir)} goes back to how it was before this fix. The backup is consumed.`
                                : `${restorable.length} files go back to how they were before this run. Their backups are consumed. Files whose backup is gone are skipped.`,
                          })
                        }
                      >
                        <ArchiveRestore className="h-3.5 w-3.5" aria-hidden />
                        Restore{" "}
                        {restorable.length === 1
                          ? "original"
                          : `all ${restorable.length}`}
                      </Button>
                    ) : null}
                  </div>
                  <ul className="divide-y divide-ink-700/40">
                    {run.entries.map((entry) => (
                      <li
                        key={`${entry.iniPath}-${entry.fixedAt}`}
                        className="flex flex-wrap items-center justify-between gap-3 px-4 py-2.5"
                      >
                        <div className="min-w-0">
                          <p
                            className="truncate font-mono text-[10px] text-zinc-400"
                            title={entry.iniPath}
                          >
                            {entry.folder} · {shortPath(entry.iniPath, modsDir)}
                          </p>
                          <p className="mt-1 font-mono text-[10px] text-zinc-600">
                            {entry.updated} hash
                            {entry.updated === 1 ? "" : "es"}
                            {entry.additions > 0
                              ? ` · ${entry.additions} section${entry.additions === 1 ? "" : "s"}`
                              : ""}
                            {" · "}
                            <span
                              className={cn(
                                entry.backupAvailable && !entry.restoredAt
                                  ? "text-zinc-500"
                                  : "text-amber-300/80",
                              )}
                            >
                              {restoreStatus(entry)}
                            </span>
                          </p>
                        </div>
                        {entry.backupAvailable && !entry.restoredAt ? (
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() =>
                              setPendingRestore({
                                entries: [entry],
                                title: "Restore this file?",
                                description: `${shortPath(entry.iniPath, modsDir)} goes back to how it was before this fix. The backup is consumed.`,
                              })
                            }
                          >
                            <ArchiveRestore className="h-3.5 w-3.5" aria-hidden />
                            Restore
                          </Button>
                        ) : null}
                      </li>
                    ))}
                  </ul>
                </li>
              );
            })}
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

      <ConfirmDialog
        open={pendingRestore !== null}
        title={pendingRestore?.title ?? "Restore the original?"}
        description={pendingRestore?.description ?? ""}
        confirmLabel={restoring ? "Restoring..." : "Restore"}
        destructive
        busy={restoring}
        onConfirm={() => {
          if (pendingRestore) void runRestore(pendingRestore.entries);
        }}
        onCancel={() => setPendingRestore(null)}
      />
    </div>
  );
}

export { cn };
