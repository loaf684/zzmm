import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import type { Settings, SettingsView } from "@/types/settings";
import { toast } from "@/store/toasts";

interface SettingsStore {
  view: SettingsView | null;
  isLoading: boolean;
  isSaving: boolean;
  loadError: string | null;
  load: () => Promise<void>;
  save: (next: Settings) => Promise<boolean>;
}

export const useSettingsStore = create<SettingsStore>((set) => ({
  view: null,
  isLoading: false,
  isSaving: false,
  loadError: null,

  load: async () => {
    set({ isLoading: true, loadError: null });
    try {
      const view = await api.settings.get();
      set({ view, isLoading: false });
    } catch (error) {
      set({ isLoading: false, loadError: toUserMessage(error) });
    }
  },

  save: async (next) => {
    set({ isSaving: true });
    try {
      const view = await api.settings.save(next);
      set({ view, isSaving: false });
      return true;
    } catch (error) {
      set({ isSaving: false });
      toast.error("Could not save settings", toUserMessage(error));
      return false;
    }
  },
}));
