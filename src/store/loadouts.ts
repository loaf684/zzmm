import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import { remindReload } from "@/lib/reload";
import { useModsStore } from "@/store/mods";
import { toast } from "@/store/toasts";
import type { LoadoutSummary } from "@/types/mods";

interface LoadoutsStore {
  loadouts: LoadoutSummary[];
  isLoading: boolean;
  isSaving: boolean;
  busyName: string | null;
  error: string | null;

  load: () => Promise<void>;
  save: (name: string) => Promise<boolean>;
  remove: (name: string) => Promise<void>;
  apply: (name: string) => Promise<boolean>;
}

function upsert(
  loadouts: LoadoutSummary[],
  summary: LoadoutSummary,
): LoadoutSummary[] {
  const next = loadouts.filter(
    (item) => item.name.toLowerCase() !== summary.name.toLowerCase(),
  );
  next.push(summary);
  return next.sort((a, b) => a.name.localeCompare(b.name));
}

export const useLoadoutsStore = create<LoadoutsStore>((set, get) => ({
  loadouts: [],
  isLoading: false,
  isSaving: false,
  busyName: null,
  error: null,

  load: async () => {
    if (get().isLoading) return;
    set({ isLoading: true, error: null });

    try {
      const loadouts = await api.mods.listLoadouts();
      set({ loadouts, isLoading: false });
    } catch (error) {
      set({ isLoading: false, error: toUserMessage(error) });
    }
  },

  save: async (name) => {
    if (get().isSaving) return false;
    set({ isSaving: true });

    try {
      const summary = await api.mods.saveLoadout(name);
      set((state) => ({
        isSaving: false,
        loadouts: upsert(state.loadouts, summary),
      }));
      toast.success(
        "Loadout saved",
        `${summary.name} stores ${summary.entryCount} mod${
          summary.entryCount === 1 ? "" : "s"
        }.`,
      );
      return true;
    } catch (error) {
      set({ isSaving: false });
      toast.error("Could not save loadout", toUserMessage(error));
      return false;
    }
  },

  remove: async (name) => {
    if (get().busyName) return;
    set({ busyName: name });

    try {
      await api.mods.deleteLoadout(name);
      set((state) => ({
        busyName: null,
        loadouts: state.loadouts.filter(
          (item) => item.name.toLowerCase() !== name.toLowerCase(),
        ),
      }));
      toast.success("Loadout deleted", `${name} was removed.`);
    } catch (error) {
      set({ busyName: null });
      toast.error("Could not delete loadout", toUserMessage(error));
    }
  },

  apply: async (name) => {
    if (get().busyName) return false;
    set({ busyName: name });

    try {
      const result = await api.mods.applyLoadout(name);
      useModsStore.getState().setResult(result.scan);

      if (result.failed.length > 0) {
        toast.error(
          "Some mods could not be switched",
          result.failed[0]?.message,
        );
      }

      if (result.missing.length > 0) {
        toast.info(
          "Some saved mods were not found",
          `${result.missing.length} mod${
            result.missing.length === 1 ? "" : "s"
          } in this loadout no longer exist.`,
        );
      }

      if (result.changed.length > 0) {
        toast.success(
          `${name} applied`,
          `${result.changed.length} mod${
            result.changed.length === 1 ? "" : "s"
          } switched${result.backupFile ? " · previous state backed up" : ""}.`,
        );
        remindReload();
      } else if (result.missing.length === 0) {
        toast.info("Already up to date", `${name} matches the current setup.`);
      }

      set({ busyName: null });
      return result.failed.length === 0;
    } catch (error) {
      set({ busyName: null });
      toast.error("Could not apply loadout", toUserMessage(error));
      return false;
    }
  },
}));
