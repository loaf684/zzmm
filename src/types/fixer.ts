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
  /**
   * True when every remaining recommendation is ignored one by one. The file
   * stays listed so those fixes can be seen and unignored; nothing is written.
   */
  ignored: boolean;
  /**
   * `fix` when every remaining recommendation is ignored one by one. A file or
   * mod ignore is never reported: those rows are hidden from the scan.
   */
  ignoredScope?: IgnoreScope | null;
  /** Set when the file could not be read; nothing was written for it. */
  error?: string;
}

export interface FixerScanResult {
  scanned: number;
  skipped: number;
  /** Inis hidden because the ignore list covers the mod or the file. */
  ignored: number;
  files: FixerFile[];
}

export interface FixerApplied {
  iniPath: string;
  folder: string;
  character: string;
  /** Unix seconds of the run. */
  fixedAt: number;
  /**
   * True when this file's pre-fix bytes are in its mod's snapshot, so undoing
   * the mod puts it back. False when no snapshot was asked for or storing it
   * failed.
   */
  snapshot: boolean;
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

export interface FixerUndoResult {
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

/**
 * The one restorable state a mod has: how it was right before its most recent
 * fix. A new fix replaces it instead of adding another.
 */
export interface FixerSnapshotEntry {
  folder: string;
  /** Unix seconds the originals were read. */
  capturedAt: number;
  /** How many inis an undo puts back. */
  files: number;
  iniPaths: string[];
}

/** The ignore list and the mods an undo can still put back. */
export interface FixerStateView {
  ignored: FixerIgnoredEntry[];
  /** Newest capture first. */
  snapshots: FixerSnapshotEntry[];
}