import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import { toast } from "@/store/toasts";
import type {
  FixerHistoryEntry,
  FixerIgnoredEntry,
  FixerStateView,
} from "@/types/fixer";
import type { FixerIgnoreTarget } from "@/lib/api/fixer";

interface FixerStore {
  /** Persisted ignore list, newest entry last. */
  ignored: FixerIgnoredEntry[];
  /** Persisted fix history, newest first. */
  history: FixerHistoryEntry[];
  isLoading: boolean;
  isBusy: boolean;
  /** Set when the state file could not be read, so the lists are incomplete. */
  loadError: string | null;

  load: () => Promise<void>;
  setIgnored: (
    target: FixerIgnoreTarget,
    ignored: boolean,
  ) => Promise<FixerStateView | null>;
}

/**
 * The ignore list and the fix history, backed by `fixer_state`. The page reads
 * this on mount, so both survive navigating to another tab and back (and an app
 * restart: the backend keeps them in the config folder).
 */
export const useFixerStore = create<FixerStore>((set) => ({
  ignored: [],
  history: [],
  isLoading: false,
  isBusy: false,
  loadError: null,

  load: async () => {
    set({ isLoading: true });
    try {
      const view = await api.fixer.state();
      set({
        ignored: view.ignored,
        history: view.history,
        isLoading: false,
        loadError: null,
      });
    } catch (error) {
      set({ isLoading: false, loadError: toUserMessage(error) });
    }
  },

  setIgnored: async (target, ignored) => {
    set({ isBusy: true });
    try {
      const view = await api.fixer.setIgnored(target, ignored);
      set({
        ignored: view.ignored,
        history: view.history,
        isLoading: false,
        loadError: null,
        isBusy: false,
      });
      return view;
    } catch (error) {
      set({ isBusy: false });
      toast.error(
        ignored ? "Could not ignore that" : "Could not unignore that",
        toUserMessage(error),
      );
      return null;
    }
  },
}));
