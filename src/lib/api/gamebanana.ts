import { call } from "@/lib/api/client";
import type {
  BrowsePage,
  BrowseParams,
  CategoryChip,
  DownloadOutcome,
  ModDetail,
  UpdateCheckResult,
} from "@/types/gamebanana";

export const gamebananaApi = {
  browse(params: BrowseParams): Promise<BrowsePage> {
    return call<BrowsePage>("browse_gamebanana", { params });
  },

  detail(modId: number): Promise<ModDetail> {
    return call<ModDetail>("get_gamebanana_mod", { modId });
  },

  categories(): Promise<CategoryChip[]> {
    return call<CategoryChip[]>("get_gamebanana_categories");
  },

  install(
    modId: number,
    fileId: number | null,
    replaceFolder: string | null = null,
  ): Promise<DownloadOutcome> {
    return call<DownloadOutcome>("install_gamebanana_mod", {
      modId,
      fileId,
      replaceFolder,
    });
  },

  checkUpdates(): Promise<UpdateCheckResult> {
    return call<UpdateCheckResult>("check_mod_updates");
  },

  /** Stop one GameBanana download; other running downloads keep going. */
  cancelInstall(modId: number): Promise<boolean> {
    return call<boolean>("cancel_gamebanana_install", { modId });
  },
};
