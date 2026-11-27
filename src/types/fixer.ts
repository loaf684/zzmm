/** One `hash = ...` rewrite the backend would make. */
export interface FixEdit {
  /** 1-based line number in the INI. */
  line: number;
  from: string;
  to: string;
}

export interface FixFile {
  relativePath: string;
  edits: FixEdit[];
}

export interface FixFailure {
  relativePath: string;
  message: string;
}

/** Which game version the bundled hash table targets. */
export interface FixTableInfo {
  targetGameVersion: string;
  source: string;
  sourceUrl: string;
  sourceCommit: string;
  generated: string;
  renames: number;
}

/** Read-only result: what a fix would change, plus restorable backups. */
export interface FixScan {
  folderName: string;
  table: FixTableInfo;
  files: FixFile[];
  edits: number;
  backups: string[];
  failures: FixFailure[];
}

export interface FixApplyResult {
  folderName: string;
  table: FixTableInfo;
  edits: number;
  written: string[];
  failures: FixFailure[];
}

export interface FixRestoreResult {
  folderName: string;
  restored: string[];
  failures: FixFailure[];
}