import { useEffect } from "react";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";

import { AppShell } from "@/components/layout/AppShell";
import { BrowsePage } from "@/features/browse/BrowsePage";
import { ModDetailPage } from "@/features/browse/ModDetailPage";
import { InstalledPage } from "@/features/installed/InstalledPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import {
  onDownloadProgress,
  onInstallProgress,
  onModScanProgress,
} from "@/lib/api/events";
import { useDownloadStore } from "@/store/download";
import { useInstallerStore } from "@/store/installer";
import { useModsStore } from "@/store/mods";
import { useSettingsStore } from "@/store/settings";

export default function App() {
  // Load persisted settings once; every feature reads from this store.
  useEffect(() => {
    void useSettingsStore.getState().load();
  }, []);

  // Scan progress is global UI state; keep the listener alive for the whole app.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void onModScanProgress((progress) => {
      useModsStore.getState().setProgress(progress);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Browser-only preview: the Tauri event bridge is unavailable.
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Installer progress feeds the global install indicator in the app shell.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void onInstallProgress((progress) => {
      useInstallerStore.getState().setProgress(progress);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Browser-only preview: the Tauri event bridge is unavailable.
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // GameBanana download/install progress, shown on the mod detail page.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void onDownloadProgress((progress) => {
      useDownloadStore.getState().setProgress(progress);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Browser-only preview: the Tauri event bridge is unavailable.
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return (
    <HashRouter>
      <Routes>
        <Route element={<AppShell />}>
          <Route index element={<Navigate to="/installed" replace />} />
          <Route path="/installed" element={<InstalledPage />} />
          <Route path="/browse" element={<BrowsePage />} />
          <Route path="/browse/:modId" element={<ModDetailPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          {/* Old dedicated pages now live inside the mod detail panel. */}
          <Route path="/hotkeys" element={<Navigate to="/installed" replace />} />
          <Route path="/preview" element={<Navigate to="/installed" replace />} />
          <Route path="*" element={<Navigate to="/installed" replace />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
