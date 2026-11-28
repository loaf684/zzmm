/** Types mirroring `src-tauri/src/mods/model.rs` and `metadata.rs`. */

import type { IniSettingValue, IniVariable } from "@/types/ini";

export interface ModMetadata {
  source: string | null;
  gameBananaModId: number | null;
  gameBananaFileId: number | null;
  name: string | null;
  character: string | null;
  version: string | null;
  author: string | null;
  installedAt: string | null;
  thumbnailUrl: string | null;
  notes: string | null;
}

export interface InstalledMod {
  folderName: string;
  baseName: string;
  displayName: string;
  character: string;
  enabled: boolean;
  linked: boolean;
  thumbnailPath: string | null;
  hashes: string[];
  hotkeyCount: number;
  hasConflict: boolean;
  metadata: ModMetadata | null;
}

/** Extra data loaded when the installed-mod detail panel opens. */
export interface ModDetails {
  folderName: string;
  /** Absolute paths to root-level preview images (asset protocol). */
  previews: string[];
  folderSize: number;
  installedAt: string | null;
  notes: string | null;
}

/** One active `[Key*]` binding inside a mod. */
export interface ModHotkey {
  file: string;
  section: string;
  line: number;
  /** Actual spelling of the `key` setting in the file. */
  keySetting: string | null;
  /** Actual spelling of the `type` setting, if present. */
  typeSetting: string | null;
  binding: string;
  actionType: string;
  tokens: string[];
  options: number;
  condition: string | null;
  description: string | null;
  variables: IniVariable[];
  settings: IniSettingValue[];
}

export interface HashConflictPart {
  folderName: string;
  baseName: string;
  displayName: string;
  file: string;
  section: string;
  line: number;
}

export interface HashConflict {
  hash: string;
  mods: HashConflictPart[];
}

export interface ModScanResult {
  modsDir: string;
  mods: InstalledMod[];
  conflicts: HashConflict[];
  enabledCount: number;
  disabledCount: number;
  unlinkedCount: number;
}

export interface ModScanProgress {
  scanned: number;
  total: number;
  folderName: string;
}

/** One requested enable/disable change for a batch operation. */
export interface ModStateUpdate {
  folderName: string;
  enabled: boolean;
}

export interface ModStateFailure {
  folderName: string;
  message: string;
}

export interface ApplyModStatesResult {
  scan: ModScanResult;
  changed: string[];
  failed: ModStateFailure[];
  backupFile: string | null;
}

export interface LoadoutSummary {
  name: string;
  entryCount: number;
  enabledCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface LoadoutApplyResult {
  scan: ModScanResult;
  changed: string[];
  /** Loadout entries whose mod folder no longer exists. */
  missing: string[];
  failed: ModStateFailure[];
  backupFile: string | null;
}
