import { create } from "zustand";

import { api } from "@/lib/api";
import { ApiError } from "@/lib/api/client";
import { toUserMessage } from "@/lib/errors";
import { useModsStore } from "@/store/mods";
import { toast } from "@/store/toasts";
import type { DownloadProgress } from "@/types/gamebanana";

interface InstallTarget {
  modId: number;
  name: string;
  fileId?: number | null;
  /** Existing folder being updated; disabled after a successful install. */
  replaceFolder?: string | null;
}

/** One in-flight GameBanana download. Keyed by mod ID so unrelated mods never
 *  share progress, errors, or a cancel request. */
export interface DownloadJob {
  name: string;
  cancelling: boolean;
  progress: DownloadProgress | null;
}

interface DownloadStore {
  /** Active downloads by GameBanana mod ID. */
  jobs: Record<number, DownloadJob>;
  install: (target: InstallTarget) => Promise<boolean>;
  cancel: (modId: number) => Promise<void>;
  setProgress: (progress: DownloadProgress | null) => void;
}

export const useDownloadStore = create<DownloadStore>((set, get) => ({
  jobs: {},

  setProgress: (progress) => {
    if (!progress) return;

    set((state) => {
      const job = state.jobs[progress.modId];
      // Late events for a download that already settled are dropped.
      if (!job) return {};

      return {
        jobs: {
          ...state.jobs,
          [progress.modId]: { ...job, progress },
        },
      };
    });
  },

  install: async (target) => {
    // The check and the reservation are synchronous, so a double click on the
    // same mod cannot start two downloads. Other mods stay startable.
    if (get().jobs[target.modId]) return false;

    set((state) => ({
      jobs: {
        ...state.jobs,
        [target.modId]: {
          name: target.name,
          cancelling: false,
          progress: {
            modId: target.modId,
            name: target.name,
            stage: "downloading",
            percent: 0,
            received: 0,
            total: 0,
            message: "Preparing download...",
          },
        },
      },
    }));

    try {
      const outcome = await api.gamebanana.install(
        target.modId,
        target.fileId ?? null,
        target.replaceFolder ?? null,
      );

      const where = outcome.installed.join(", ");
      toast.success(
        `${outcome.modName} installed`,
        outcome.replacedFolder
          ? `${where}. The previous version "${outcome.replacedFolder}" was disabled.`
          : where || undefined,
      );

      if (outcome.warnings.length > 0) {
        toast.info("Installed with warnings", outcome.warnings[0]);
      }

      // Refresh the Installed page with the new folder(s).
      await useModsStore.getState().scan();
      return true;
    } catch (error) {
      if (error instanceof ApiError && error.code === "install_cancelled") {
        toast.info("Installation cancelled", "No mod folder was installed.");
        return false;
      }

      toast.error("Install failed", toUserMessage(error));
      return false;
    } finally {
      set((state) => {
        const jobs = { ...state.jobs };
        delete jobs[target.modId];
        return { jobs };
      });
    }
  },

  cancel: async (modId) => {
    const job = get().jobs[modId];
    if (!job || job.cancelling) return;

    set({
      jobs: {
        ...get().jobs,
        [modId]: { ...job, cancelling: true },
      },
    });

    try {
      await api.gamebanana.cancelInstall(modId);
    } catch (error) {
      const current = get().jobs[modId];
      if (current) {
        set({
          jobs: {
            ...get().jobs,
            [modId]: { ...current, cancelling: false },
          },
        });
      }
      toast.error("Could not cancel the download", toUserMessage(error));
    }
  },
}));