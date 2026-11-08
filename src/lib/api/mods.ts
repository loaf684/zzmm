import { call } from "@/lib/api/client";
import type {
  ApplyModStatesResult,
  CharacterAssignResult,
  LoadoutApplyResult,
  LoadoutSummary,
  ModDetails,
  ModScanResult,
  ModStateUpdate,
} from "@/types/mods";

export const modsApi = {
  scan(): Promise<ModScanResult> {
    return call<ModScanResult>("scan_mods");
  },

  setEnabled(folderName: string, enabled: boolean): Promise<ModScanResult> {
    return call<ModScanResult>("set_mod_enabled", { folderName, enabled });
  },

  remove(folderName: string): Promise<ModScanResult> {
    return call<ModScanResult>("delete_mod", { folderName });
  },

  /** `null` opens the Mods folder itself. */
  openFolder(folderName: string | null = null): Promise<void> {
    return call<void>("open_mod_folder", { folderName });
  },

  getDetails(folderName: string): Promise<ModDetails> {
    return call<ModDetails>("get_mod_details", { folderName });
  },

  setNotes(folderName: string, notes: string): Promise<ModScanResult> {
    return call<ModScanResult>("set_mod_notes", { folderName, notes });
  },

  rename(folderName: string, newName: string): Promise<ModScanResult> {
    return call<ModScanResult>("rename_mod", { folderName, newName });
  },

  /** `null` means the native picker was cancelled. */
  changePreview(folderName: string): Promise<ModScanResult | null> {
    return call<ModScanResult | null>("change_mod_preview", { folderName });
  },

  openUrl(url: string): Promise<void> {
    return call<void>("open_external_url", { url });
  },

  applyStates(updates: ModStateUpdate[]): Promise<ApplyModStatesResult> {
    return call<ApplyModStatesResult>("apply_mod_states", { updates });
  },

  setCharacter(
    folderName: string,
    character: string | null,
  ): Promise<ModScanResult> {
    return call<ModScanResult>("set_mod_character", { folderName, character });
  },

  /** Tag several freshly installed folders in one backend pass. */
  assignCharacter(
    folderNames: string[],
    character: string | null,
  ): Promise<CharacterAssignResult> {
    return call<CharacterAssignResult>("assign_mods_character", {
      folderNames,
      character,
    });
  },

  listLoadouts(): Promise<LoadoutSummary[]> {
    return call<LoadoutSummary[]>("list_loadouts");
  },

  saveLoadout(name: string): Promise<LoadoutSummary> {
    return call<LoadoutSummary>("save_loadout", { name });
  },

  deleteLoadout(name: string): Promise<void> {
    return call<void>("delete_loadout", { name });
  },

  applyLoadout(name: string): Promise<LoadoutApplyResult> {
    return call<LoadoutApplyResult>("apply_loadout", { name });
  },
};
