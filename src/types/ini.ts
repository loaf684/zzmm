/** Types mirroring `src-tauri/src/ini_parser/model.rs`. */

export interface IniFileInfo {
  relativePath: string;
  name: string;
  size: number;
  hotkeyCount: number;
}

export interface IniSectionView {
  name: string;
  line: number;
  settingCount: number;
  isHotkey: boolean;
}

export interface IniVariable {
  name: string;
  value: string;
  line: number;
}

export interface IniSettingValue {
  key: string;
  value: string;
  line: number;
}

export interface HotkeyView {
  section: string;
  line: number;
  /** Actual spelling of the `key` setting in the file. */
  keySetting: string | null;
  typeSetting: string | null;
  binding: string;
  /** activate | hold | toggle | cycle | custom */
  actionType: string;
  hasKey: boolean;
  tokens: string[];
  variables: IniVariable[];
  settings: IniSettingValue[];
}

export interface IniDocument {
  modFolder: string;
  relativePath: string;
  absolutePath: string;
  encoding: string;
  content: string;
  lineCount: number;
  size: number;
  sections: IniSectionView[];
  hotkeys: HotkeyView[];
  backupPath: string | null;
}

export interface HotkeyConflictEntry {
  folderName: string;
  baseName: string;
  displayName: string;
  file: string;
  section: string;
  line: number;
  binding: string;
  actionType: string;
}

export interface HotkeyConflict {
  key: string;
  binding: string;
  entries: HotkeyConflictEntry[];
}

export interface HotkeyConflictReport {
  scannedMods: number;
  scannedFiles: number;
  hotkeys: number;
  conflicts: HotkeyConflict[];
}
