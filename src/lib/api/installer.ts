import { call } from "@/lib/api/client";
import type { InstallBatchResult } from "@/types/installer";

export const installerApi = {
  installArchives(paths: string[]): Promise<InstallBatchResult> {
    return call<InstallBatchResult>("install_archives", { paths });
  },

  cancelInstall(): Promise<boolean> {
    return call<boolean>("cancel_install");
  },
};
