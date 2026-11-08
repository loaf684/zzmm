import { call } from "@/lib/api/client";
import type {
  DetectedModsDir,
  ModsDirState,
  Settings,
  SettingsView,
} from "@/types/settings";

export const settingsApi = {
  get(): Promise<SettingsView> {
    return call<SettingsView>("get_settings");
  },

  save(settings: Settings): Promise<SettingsView> {
    return call<SettingsView>("save_settings", { settings });
  },

  /** Cheap existence/shape check used while the user edits the path. */
  validateModsDir(path: string): Promise<ModsDirState> {
    return call<ModsDirState>("validate_mods_dir", { path });
  },

  /** Native folder picker. Returns `null` when the user cancels. */
  chooseModsFolder(): Promise<string | null> {
    return call<string | null>("choose_mods_folder");
  },

  detectModsDirs(): Promise<DetectedModsDir[]> {
    return call<DetectedModsDir[]>("detect_mods_dirs");
  },
};
