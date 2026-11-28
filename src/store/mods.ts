import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import { remindReload } from "@/lib/reload";
import { useHotkeysStore } from "@/store/hotkeys";
import { useSettingsStore } from "@/store/settings";
import { toast } from "@/store/toasts";
import type {
  ModDetails,
  ModHotkey,
  ModScanProgress,
  ModScanResult,
  ModStateUpdate,
} from "@/types/mods";

interface ModsStore {
  result: ModScanResult | null;
  isScanning: boolean;
  progress: ModScanProgress | null;
  scanError: string | null;
  busyFolder: string | null;

  details: Record<string, ModDetails>;
  detailLoading: string | null;
  detailError: string | null;
  hotkeys: Record<string, ModHotkey[]>;
  hotkeyLoading: string | null;
  hotkeyError: string | null;

  scan: () => Promise<void>;
  setResult: (result: ModScanResult | null) => void;
  setEnabled: (folderName: string, enabled: boolean) => Promise<void>;
  applyStates: (updates: ModStateUpdate[]) => Promise<boolean>;
  setCharacter: (folderName: string, character: string | null) => Promise<boolean>;
  remove: (folderName: string) => Promise<void>;
  openFolder: (folderName?: string) => Promise<void>;
  setProgress: (progress: ModScanProgress | null) => void;

  loadDetails: (folderName: string, force?: boolean) => Promise<void>;
  loadHotkeys: (folderName: string, force?: boolean) => Promise<void>;
  /** Writes several edited bindings at once, then reloads and scans only once. */
  saveHotkeys: (
    folderName: string,
    edits: Array<{ hotkey: ModHotkey; binding: string; actionType: string }>,
  ) => Promise<boolean>;
  saveNotes: (folderName: string, notes: string) => Promise<boolean>;
  rename: (folderName: string, newName: string) => Promise<string | null>;
  changePreview: (folderName: string) => Promise<boolean>;
  openUrl: (url: string) => Promise<void>;
}

/** Backend `rename` strips a typed `DISABLED_` prefix; mirror that here. */
function stripDisabledPrefix(name: string): string {
  return name.trim().replace(/^DISABLED_/i, "").trim();
}

export const useModsStore = create<ModsStore>((set, get) => ({
  result: null,
  isScanning: false,
  progress: null,
  scanError: null,
  busyFolder: null,

  details: {},
  detailLoading: null,
  detailError: null,
  hotkeys: {},
  hotkeyLoading: null,
  hotkeyError: null,

  scan: async () => {
    // React StrictMode mounts effects twice in development; never run two scans.
    if (get().isScanning) return;

    const modsDir = useSettingsStore.getState().view?.settings.modsDir ?? null;
    if (!modsDir) {
      set({ result: null, scanError: null, progress: null });
      return;
    }

    set({
      isScanning: true,
      scanError: null,
      progress: { scanned: 0, total: 0, folderName: "" },
    });

    try {
      const result = await api.mods.scan();
      set({ result, isScanning: false, progress: null });
    } catch (error) {
      set({ isScanning: false, scanError: toUserMessage(error), progress: null });
    }
  },

  setEnabled: async (folderName, enabled) => {
    if (get().busyFolder) return;

    const settings = useSettingsStore.getState().view?.settings;
    const mods = get().result?.mods ?? [];

    // One-active-per-character: enabling a mod disables the other enabled mods
    // for that character in the same batch (and writes a safety backup).
    if (enabled && settings?.oneActivePerCharacter) {
      const target = mods.find((mod) => mod.folderName === folderName);
      const character = target?.character.trim() ?? "";
      const siblings =
        target && target.character.trim() !== "" && character !== "Uncategorized"
          ? mods.filter(
              (mod) =>
                mod.enabled &&
                mod.folderName !== folderName &&
                mod.character.trim() === character,
            )
          : [];

      if (siblings.length > 0) {
        set({ busyFolder: folderName });

        try {
          const result = await api.mods.applyStates([
            { folderName, enabled: true },
            ...siblings.map((mod) => ({
              folderName: mod.folderName,
              enabled: false,
            })),
          ]);
          set({ result: result.scan, busyFolder: null, progress: null });

          if (result.failed.length > 0) {
            toast.error(
              "Some mods could not be updated",
              result.failed[0]?.message,
            );
          } else {
            toast.success(
              "Mod enabled",
              `Disabled ${siblings.length} other ${character} mod${
                siblings.length === 1 ? "" : "s"
              } to keep one active.`,
            );
          }
          if (result.changed.length > 0) remindReload();
        } catch (error) {
          set({ busyFolder: null, progress: null });
          toast.error("Could not update mod", toUserMessage(error));
        }
        return;
      }
    }

    set({ busyFolder: folderName });

    try {
      const result = await api.mods.setEnabled(folderName, enabled);
      set({ result, busyFolder: null, progress: null });
      toast.success(
        enabled ? "Mod enabled" : "Mod disabled",
        enabled
          ? "The DISABLED_ prefix was removed."
          : "The folder was renamed with the DISABLED_ prefix.",
      );
      remindReload();
    } catch (error) {
      set({ busyFolder: null, progress: null });
      toast.error("Could not update mod", toUserMessage(error));
    }
  },

  setResult: (result) => set({ result }),

  applyStates: async (updates) => {
    if (get().busyFolder || updates.length === 0) return false;
    set({ busyFolder: updates[0]?.folderName ?? null });

    try {
      const result = await api.mods.applyStates(updates);
      set({ result: result.scan, busyFolder: null, progress: null });

      if (result.failed.length > 0) {
        toast.error(
          "Some mods could not be updated",
          result.failed[0]?.message,
        );
      }

      if (result.changed.length > 0) {
        toast.success(
          `${result.changed.length} mod${
            result.changed.length === 1 ? "" : "s"
          } updated`,
          result.backupFile
            ? "The previous enabled state was backed up."
            : "Enabled state saved.",
        );
        remindReload();
      }

      return result.failed.length === 0;
    } catch (error) {
      set({ busyFolder: null, progress: null });
      toast.error("Could not update mods", toUserMessage(error));
      return false;
    }
  },

  setCharacter: async (folderName, character) => {
    try {
      const result = await api.mods.setCharacter(folderName, character);
      set({ result });
      const trimmed = character?.trim() ?? "";
      toast.success(
        "Character updated",
        trimmed
          ? `Assigned to ${trimmed}.`
          : "Reset to automatic character detection.",
      );
      return true;
    } catch (error) {
      toast.error("Could not update character", toUserMessage(error));
      return false;
    }
  },

  remove: async (folderName) => {
    if (get().busyFolder) return;
    set({ busyFolder: folderName });

    try {
      const result = await api.mods.remove(folderName);
      set({ result, busyFolder: null, progress: null });
      toast.success("Mod deleted", "The mod folder was permanently removed.");
      remindReload();
    } catch (error) {
      set({ busyFolder: null, progress: null });
      toast.error("Could not delete mod", toUserMessage(error));
    }
  },

  openFolder: async (folderName) => {
    try {
      await api.mods.openFolder(folderName ?? null);
    } catch (error) {
      toast.error("Could not open folder", toUserMessage(error));
    }
  },

  loadDetails: async (folderName, force = false) => {
    if (!force && get().details[folderName]) return;
    if (get().detailLoading === folderName) return;

    set({ detailLoading: folderName, detailError: null });

    try {
      const details = await api.mods.getDetails(folderName);
      set((state) => ({
        detailLoading: state.detailLoading === folderName ? null : state.detailLoading,
        details: { ...state.details, [folderName]: details },
      }));
    } catch (error) {
      set((state) => ({
        detailLoading: state.detailLoading === folderName ? null : state.detailLoading,
        detailError: toUserMessage(error),
      }));
    }
  },

  loadHotkeys: async (folderName, force = false) => {
    if (!force && get().hotkeys[folderName]) return;
    if (get().hotkeyLoading === folderName) return;

    set({ hotkeyLoading: folderName, hotkeyError: null });

    try {
      const hotkeys = await api.ini.getModHotkeys(folderName);
      set((state) => ({
        hotkeyLoading: state.hotkeyLoading === folderName ? null : state.hotkeyLoading,
        hotkeys: { ...state.hotkeys, [folderName]: hotkeys },
      }));
    } catch (error) {
      set((state) => ({
        hotkeyLoading: state.hotkeyLoading === folderName ? null : state.hotkeyLoading,
        hotkeyError: toUserMessage(error),
      }));
    }
  },

  saveHotkeys: async (folderName, edits) => {
    if (edits.length === 0) return true;

    const invalid = edits.find((edit) => !edit.binding.trim());
    if (invalid) {
      toast.error(
        "Hotkey cannot be empty",
        `Capture a shortcut for ${invalid.hotkey.section} first.`,
      );
      return false;
    }

    // One payload for the whole batch: the backend reads and rewrites each INI
    // once instead of once per changed field.
    const batch = edits.flatMap((edit) => {
      const binding = edit.binding.trim();
      const actionType = edit.actionType.trim();
      const bindingChanged = binding !== edit.hotkey.binding;
      const typeChanged =
        Boolean(actionType) && actionType !== edit.hotkey.actionType;

      if (!bindingChanged && !typeChanged) return [];

      return [
        {
          file: edit.hotkey.file,
          section: edit.hotkey.section,
          keySetting: edit.hotkey.keySetting ?? "key",
          typeSetting: edit.hotkey.typeSetting ?? "type",
          binding: bindingChanged ? binding : null,
          actionType: typeChanged ? actionType : null,
        },
      ];
    });

    if (batch.length === 0) return true;

    try {
      const writtenFiles = await api.ini.applyHotkeyEdits(folderName, batch);

      // Re-read once for the whole batch so the panel and conflict scan see the
      // new bindings without paying a reload per hotkey.
      await get().loadHotkeys(folderName, true);

      const enabled =
        get().result?.mods.find((mod) => mod.folderName === folderName)?.enabled ??
        false;
      if (enabled) await useHotkeysStore.getState().scanConflicts();

      remindReload();
      toast.success(
        `${edits.length} hotkey${edits.length === 1 ? "" : "s"} saved`,
        `${writtenFiles} INI file${writtenFiles === 1 ? "" : "s"} rewritten in one pass.`,
      );
      return true;
    } catch (error) {
      toast.error("Could not save hotkeys", toUserMessage(error));
      return false;
    }
  },

  saveNotes: async (folderName, notes) => {
    try {
      const result = await api.mods.setNotes(folderName, notes);
      const trimmed = notes.trim();
      set((state) => {
        const current = state.details[folderName];
        return {
          result,
          details: current
            ? {
                ...state.details,
                [folderName]: {
                  ...current,
                  notes: trimmed.length > 0 ? trimmed : null,
                },
              }
            : state.details,
        };
      });
      toast.success("Notes saved", "Stored in the mod's .zzmm.json sidecar.");
      return true;
    } catch (error) {
      toast.error("Could not save notes", toUserMessage(error));
      return false;
    }
  },

  rename: async (folderName, newName) => {
    try {
      const result = await api.mods.rename(folderName, newName);
      set({ result });
      const base = stripDisabledPrefix(newName);
      toast.success("Mod renamed", `The folder is now ${base}.`);
      remindReload();
      return base;
    } catch (error) {
      toast.error("Could not rename mod", toUserMessage(error));
      return null;
    }
  },

  changePreview: async (folderName) => {
    try {
      const result = await api.mods.changePreview(folderName);
      if (!result) return false;

      set((state) => {
        const details = { ...state.details };
        delete details[folderName];
        return { result, details };
      });
      toast.success("Preview updated", "The new image is now shown on the card.");
      return true;
    } catch (error) {
      toast.error("Could not change preview", toUserMessage(error));
      return false;
    }
  },

  openUrl: async (url) => {
    try {
      await api.mods.openUrl(url);
    } catch (error) {
      toast.error("Could not open link", toUserMessage(error));
    }
  },

  setProgress: (progress) => set({ progress }),
}));
