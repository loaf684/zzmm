import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import { remindReload } from "@/lib/reload";
import type { FixApplyResult, FixRestoreResult, FixScan } from "@/types/fixer";

type FixerAction = "scan" | "apply" | "restore";

interface FixerEntry {
  scan: FixScan | null;
  busy: FixerAction | null;
  error: string | null;
  lastApply: FixApplyResult | null;
  lastRestore: FixRestoreResult | null;
}

interface FixerStore {
  entries: Record<string, FixerEntry>;
  scan: (folderName: string) => Promise<void>;
  apply: (folderName: string) => Promise<FixApplyResult | null>;
  restore: (folderName: string) => Promise<FixRestoreResult | null>;
  forget: (folderName: string) => void;
}

const EMPTY: FixerEntry = {
  scan: null,
  busy: null,
  error: null,
  lastApply: null,
  lastRestore: null,
};

export const useFixerStore = create<FixerStore>((set, get) => {
  const patch = (folderName: string, next: Partial<FixerEntry>) =>
    set((state) => ({
      entries: {
        ...state.entries,
        [folderName]: { ...(state.entries[folderName] ?? EMPTY), ...next },
      },
    }));

  return {
    entries: {},

    scan: async (folderName) => {
      if (get().entries[folderName]?.busy) return;

      patch(folderName, { busy: "scan", error: null });

      try {
        const scan = await api.fixer.scan(folderName);
        patch(folderName, { busy: null, scan });
      } catch (error) {
        patch(folderName, { busy: null, error: toUserMessage(error) });
      }
    },

    apply: async (folderName) => {
      if (get().entries[folderName]?.busy) return null;

      patch(folderName, { busy: "apply", error: null, lastRestore: null });

      try {
        const lastApply = await api.fixer.apply(folderName);
        patch(folderName, { busy: null, lastApply });
        if (lastApply.written.length > 0) remindReload();
        // Refresh the plan so the file list and backup state match the disk.
        await get().scan(folderName);
        return lastApply;
      } catch (error) {
        patch(folderName, { busy: null, error: toUserMessage(error) });
        return null;
      }
    },

    restore: async (folderName) => {
      if (get().entries[folderName]?.busy) return null;

      patch(folderName, { busy: "restore", error: null, lastApply: null });

      try {
        const lastRestore = await api.fixer.restoreBackups(folderName);
        patch(folderName, { busy: null, lastRestore });
        if (lastRestore.restored.length > 0) remindReload();
        await get().scan(folderName);
        return lastRestore;
      } catch (error) {
        patch(folderName, { busy: null, error: toUserMessage(error) });
        return null;
      }
    },

    forget: (folderName) =>
      set((state) => {
        const entries = { ...state.entries };
        delete entries[folderName];
        return { entries };
      }),
  };
});