import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import { remindReload } from "@/lib/reload";
import { useModsStore } from "@/store/mods";
import { toast } from "@/store/toasts";
import type { InstallProgress } from "@/types/installer";

interface InstallerStore {
  isInstalling: boolean;
  cancelling: boolean;
  progress: InstallProgress | null;
  /** Resolves with the folder names that were installed. */
  install: (paths: string[]) => Promise<string[]>;
  cancel: () => Promise<void>;
  setProgress: (progress: InstallProgress | null) => void;
}

function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export const useInstallerStore = create<InstallerStore>((set, get) => ({
  isInstalling: false,
  cancelling: false,
  progress: null,

  install: async (paths) => {
    if (get().isInstalling || paths.length === 0) return [];

    set({
      isInstalling: true,
      cancelling: false,
      progress: {
        archive: "",
        stage: "extracting",
        percent: 0,
        message: "Preparing archive…",
      },
    });

    try {
      const result = await api.installer.installArchives(paths);

      const installedFolders = result.outcomes.flatMap(
        (outcome) => outcome.installed,
      );
      const installed = installedFolders.length;
      const failures = result.outcomes.filter((outcome) => outcome.error);
      const cancelled = result.outcomes.filter((outcome) => outcome.cancelled);
      const warnings = result.outcomes.flatMap((outcome) => outcome.warnings);

      for (const failure of failures) {
        toast.error(
          `Could not install ${baseName(failure.archive)}`,
          failure.error ?? "Unknown installation error.",
        );
      }

      if (warnings.length > 0) {
        toast.info("Installed with warnings", warnings[0]);
      }

      if (installed > 0) {
        if (cancelled.length > 0) {
          toast.info(
            "Installation cancelled",
            `${installed} mod folder${installed === 1 ? "" : "s"} had already been installed.`,
          );
        } else {
          toast.success(
            `${installed} mod folder${installed === 1 ? "" : "s"} installed`,
            "Review the scan result and enable what you want to use.",
          );
        }
        remindReload("New folders are ready. Press F10 in-game to load them.");
        // Refresh the Installed page with the new folders.
        await useModsStore.getState().scan();
      } else if (cancelled.length > 0) {
        toast.info("Installation cancelled", "No changes were made.");
      } else if (failures.length === 0) {
        toast.info("Nothing installed", "The archives did not contain a mod folder.");
      }

      return installedFolders;
    } catch (error) {
      toast.error("Installation failed", toUserMessage(error));
      return [];
    } finally {
      set({ isInstalling: false, cancelling: false, progress: null });
    }
  },

  cancel: async () => {
    if (!get().isInstalling || get().cancelling) return;

    set({ cancelling: true });
    try {
      await api.installer.cancelInstall();
    } catch (error) {
      set({ cancelling: false });
      toast.error("Could not cancel installation", toUserMessage(error));
    }
  },

  setProgress: (progress) => set({ progress }),
}));
