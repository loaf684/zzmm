/** Types mirroring the fixer commands in `src-tauri` (see `fixer_scan`). */

export interface FixerHashChange {
  /** Hash written into the ini. */
  to: string;
  /** Hash that was there before. */
  from: string | null;
  /** Human label from the fixer database, e.g. "2.5: Soldier11 Body IB Hash". */
  label: string;
  /** Stable identity of this recommendation; the ignore list keys on it. */
  key: string;
  /** True when the user ignored this recommendation: reported, never written. */
  ignored: boolean;
}

export interface FixerSectionAddition {
  section: string;
  hash: string;
  label: string;
  /** Stable identity of this recommendation. */
  key: string;
  ignored: boolean;
}

export interface FixerUnsupported {
  command: string;
  hash: string;
  label: string;
}

/** What the ignore list matched: a whole mod, one ini, or one recommendation. */
export type IgnoreScope = "mod" | "file" | "fix";

export interface FixerFile {
  folder: string;
  iniPath: string;
  character: string | null;
  updated: FixerHashChange[];
  additions: FixerSectionAddition[];
  unsupported: FixerUnsupported[];
  /** The fixer database wants a buffer-level change that v1 does not apply. */
  needsBufferPass: boolean;
  /** True when an apply will not touch this file at all. */
  ignored: boolean;
  /**
   * What ignores it: `mod` (whole mod folder), `file` (this ini) or `fix`
   * (every remaining recommendation of this ini is ignored). Absent or `null`
   * when the file is not ignored.
   */
  ignoredScope?: IgnoreScope | null;
  /** Set when the file could not be read; nothing was written for it. */
  error?: string;
}

export interface FixerScanResult {
  scanned: number;
  skipped: number;
  files: FixerFile[];
}

export interface FixerApplied {
  iniPath: string;
  folder: string;
  character: string;
  /** Unix seconds of the run; with `iniPath` it identifies the history entry. */
  fixedAt: number;
  backupPath: string | null;
  /** Only the changes that were actually written. */
  updated: FixerHashChange[];
  additions: FixerSectionAddition[];
}

export interface FixerApplyResult {
  applied: FixerApplied[];
  failed: { iniPath: string; error: string }[];
  /** Requested files left untouched because the ignore list covers them. */
  ignored: string[];
}

export interface FixerRollbackResult {
  restored: string[];
  failed: { iniPath: string; error: string }[];
}

/** One entry of the persisted ignore list. */
export interface FixerIgnoredEntry {
  folder: string;
  /** Absolute ini path for a file- or fix-scoped entry, `null` for a mod. */
  iniPath: string | null;
  /** Recommendation key for a fix-scoped entry, `null` otherwise. */
  key: string | null;
  /** Display text, e.g. `S11Mod` or `S11Mod\S11.ini · 3c8697e8 → 67821d9d`. */
  label: string;
  ignoredAt: number;
}

/** One successful fix as stored, with its backup checked on disk right now. */
export interface FixerHistoryEntry {
  iniPath: string;
  folder: string;
  character: string;
  backupPath?: string | null;
  /** False when no backup was kept, it was deleted, or it was already used. */
  backupAvailable: boolean;
  fixedAt: number;
  updated: number;
  additions: number;
  restoredAt?: number | null;
}

/** The ignore list and the fix history, both persisted in the app config. */
export interface FixerStateView {
  ignored: FixerIgnoredEntry[];
  /** Newest first. */
  history: FixerHistoryEntry[];
}
