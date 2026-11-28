/**
 * Types mirroring the Rust models in `src-tauri/src/settings/model.rs`.
 * Keep both sides in sync when fields change.
 */

export interface Settings {
  schemaVersion: number;
  modsDir: string | null;
}

export interface ModsDirState {
  exists: boolean;
  isDir: boolean;
  valid: boolean;
  modCount: number;
  disabledCount: number;
  message: string | null;
}

export interface AppPaths {
  configDir: string;
  settingsFile: string;
  logDir: string;
  appVersion: string;
}

export interface SettingsView {
  settings: Settings;
  paths: AppPaths;
  modsDirState: ModsDirState;
}

export interface DetectedModsDir {
  path: string;
  label: string;
  source: string;
  modCount: number;
  disabledCount: number;
}
