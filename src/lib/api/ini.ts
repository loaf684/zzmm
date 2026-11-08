import { call } from "@/lib/api/client";
import type { HotkeyConflictReport, IniDocument, IniFileInfo } from "@/types/ini";
import type { ModHotkey } from "@/types/mods";

/** One field change in the hotkey editor; unset fields are left untouched. */
export interface HotkeyEdit {
  file: string;
  section: string;
  keySetting?: string;
  typeSetting?: string;
  binding?: string | null;
  actionType?: string | null;
}

export const iniApi = {
  listFiles(folderName: string): Promise<IniFileInfo[]> {
    return call<IniFileInfo[]>("list_mod_ini_files", { folderName });
  },

  /** Active bindings across one mod, for the installed detail panel. */
  getModHotkeys(folderName: string): Promise<ModHotkey[]> {
    return call<ModHotkey[]>("get_mod_hotkeys", { folderName });
  },

  read(folderName: string, relativePath: string): Promise<IniDocument> {
    return call<IniDocument>("read_mod_ini", { folderName, relativePath });
  },

  saveRaw(
    folderName: string,
    relativePath: string,
    content: string,
  ): Promise<IniDocument> {
    return call<IniDocument>("save_mod_ini_raw", {
      folderName,
      relativePath,
      content,
    });
  },

  updateSetting(
    folderName: string,
    relativePath: string,
    section: string,
    key: string,
    value: string,
  ): Promise<IniDocument> {
    return call<IniDocument>("update_ini_setting", {
      folderName,
      relativePath,
      section,
      key,
      value,
    });
  },

  /**
   * Write every pending hotkey edit in one call. The backend groups by file and
   * rewrites each INI once, instead of one read/write per changed field.
   */
  applyHotkeyEdits(
    folderName: string,
    edits: HotkeyEdit[],
  ): Promise<number> {
    return call<number>("apply_mod_hotkey_edits", { folderName, edits });
  },

  scanConflicts(): Promise<HotkeyConflictReport> {
    return call<HotkeyConflictReport>("scan_hotkey_conflicts");
  },
};
