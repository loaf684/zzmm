/** Types mirroring the fixer commands in `src-tauri` (see `fixer_scan`). */

export interface FixerHashChange {
  /** Hash written into the ini. */
  to: string;
  /** Hash that was there before. */
  from: string | null;
  /** Human label from the fixer database, e.g. "2.5: Soldier11 Body IB Hash". */
  label: string;
}

export interface FixerSectionAddition {
  section: string;
  hash: string;
  label: string;
}

export interface FixerUnsupported {
  command: string;
  hash: string;
  label: string;
}

export interface FixerFile {
  folder: string;
  iniPath: string;
  character: string | null;
  updated: FixerHashChange[];
  additions: FixerSectionAddition[];
  unsupported: FixerUnsupported[];
  /** The fixer database wants a buffer-level change that v1 does not apply. */
  needsBufferPass: boolean;
}

export interface FixerScanResult {
  scanned: number;
  skipped: number;
  files: FixerFile[];
}

export interface FixerApplied {
  iniPath: string;
  backupPath: string | null;
  updated: FixerHashChange[];
  additions: FixerSectionAddition[];
}

export interface FixerApplyResult {
  applied: FixerApplied[];
  failed: { iniPath: string; error: string }[];
}

export interface FixerRollbackResult {
  restored: string[];
  failed: { iniPath: string; error: string }[];
}