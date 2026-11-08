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

interface DownloadStore {
  isBusy: boolean;
  cancelling: boolean;
  activeModId: number | null;
  progress: DownloadProgress | null;
  setProgress: (progress: DownloadProgress | null) => void;
  install: (target: InstallTarget) => Promise<boolean>;
  cancel: () => Promise<void>;
}

export const useDownloadStore = create<DownloadStore>((set, get) => ({
  isBusy: false,
  cancelling: false,
  activeModId: null,
  progress: null,

  setProgress: (progress) => set({ progress }),

  install: async (target) => {
    if (get().isBusy) return false;

    set({
      isBusy: true,
      cancelling: false,
      activeModId: target.modId,
      progress: {
        modId: target.modId,
        name: target.name,
        stage: "downloading",
        percent: 0,
        received: 0,
        total: 0,
        message: "Preparing download…",
      },
    });

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
      set({ isBusy: false, cancelling: false, activeModId: null, progress: null });
    }
  },

  cancel: async () => {
    if (!get().isBusy || get().cancelling) return;

    set({ cancelling: true });
    try {
      await api.installer.cancelInstall();
    } catch (error) {
      set({ cancelling: false });
      toast.error("Could not cancel installation", toUserMessage(error));
    }
  },
}));
