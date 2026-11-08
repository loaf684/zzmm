import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import type { HotkeyConflictReport } from "@/types/ini";

interface HotkeysStore {
  conflicts: HotkeyConflictReport | null;
  isScanningConflicts: boolean;
  conflictError: string | null;

  scanConflicts: () => Promise<void>;
  clearConflicts: () => void;
}

/**
 * Conflict scanning compares every enabled mod, so it lives in its own small
 * store instead of being repeated inside the installed-mod detail panel.
 */
export const useHotkeysStore = create<HotkeysStore>((set, get) => ({
  conflicts: null,
  isScanningConflicts: false,
  conflictError: null,

  scanConflicts: async () => {
    if (get().isScanningConflicts) return;

    set({ isScanningConflicts: true, conflictError: null });

    try {
      const conflicts = await api.ini.scanConflicts();
      set({ conflicts, isScanningConflicts: false });
    } catch (error) {
      set({
        isScanningConflicts: false,
        conflictError: toUserMessage(error),
      });
    }
  },

  clearConflicts: () => set({ conflicts: null, conflictError: null }),
}));
