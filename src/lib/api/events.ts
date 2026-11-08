import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { DownloadProgress } from "@/types/gamebanana";
import type { InstallProgress } from "@/types/installer";
import type { ModScanProgress } from "@/types/mods";

export const MOD_SCAN_PROGRESS_EVENT = "mods:scan-progress";
export const INSTALL_PROGRESS_EVENT = "install:progress";
export const DOWNLOAD_PROGRESS_EVENT = "download:progress";

/** Subscribe to backend scan progress. The caller owns the returned unlistener. */
export function onModScanProgress(
  handler: (progress: ModScanProgress) => void,
): Promise<UnlistenFn> {
  return listen<ModScanProgress>(MOD_SCAN_PROGRESS_EVENT, (event) => {
    handler(event.payload);
  });
}

/** Subscribe to archive install progress. */
export function onInstallProgress(
  handler: (progress: InstallProgress) => void,
): Promise<UnlistenFn> {
  return listen<InstallProgress>(INSTALL_PROGRESS_EVENT, (event) => {
    handler(event.payload);
  });
}

/** Subscribe to GameBanana download/install progress. */
export function onDownloadProgress(
  handler: (progress: DownloadProgress) => void,
): Promise<UnlistenFn> {
  return listen<DownloadProgress>(DOWNLOAD_PROGRESS_EVENT, (event) => {
    handler(event.payload);
  });
}
