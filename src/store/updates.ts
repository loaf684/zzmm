import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import type { UpdateCheckResult } from "@/types/gamebanana";

interface UpdatesStore {
  result: UpdateCheckResult | null;
  isChecking: boolean;
  error: string | null;
  check: () => Promise<void>;
}

export const useUpdatesStore = create<UpdatesStore>((set, get) => ({
  result: null,
  isChecking: false,
  error: null,

  check: async () => {
    if (get().isChecking) return;

    set({ isChecking: true, error: null });

    try {
      const result = await api.gamebanana.checkUpdates();
      set({ result, isChecking: false });
    } catch (error) {
      set({ isChecking: false, error: toUserMessage(error) });
    }
  },
}));
