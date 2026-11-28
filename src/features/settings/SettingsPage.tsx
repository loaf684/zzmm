import { useCallback, useEffect, useRef, useState } from "react";
import { AlertTriangle, Loader2 } from "lucide-react";

import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Panel } from "@/components/ui/Panel";
import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import { useSettingsStore } from "@/store/settings";
import { toast } from "@/store/toasts";
import type { DetectedModsDir, ModsDirState } from "@/types/settings";
import { DetectedInstallations } from "./DetectedInstallations";
import { ModsFolderSection } from "./ModsFolderSection";
import { StorageSection } from "./StorageSection";

export function SettingsPage() {
  const view = useSettingsStore((state) => state.view);
  const isLoading = useSettingsStore((state) => state.isLoading);
  const isSaving = useSettingsStore((state) => state.isSaving);
  const loadError = useSettingsStore((state) => state.loadError);
  const load = useSettingsStore((state) => state.load);
  const save = useSettingsStore((state) => state.save);

  const [draft, setDraft] = useState<string | null>(null);
  const [draftState, setDraftState] = useState<ModsDirState | null>(null);
  const [validating, setValidating] = useState(false);

  const [detected, setDetected] = useState<DetectedModsDir[] | null>(null);
  const [detecting, setDetecting] = useState(false);
  const autoDetectAttempted = useRef(false);

  // The draft is the editable copy; it resets whenever persisted settings change.
  useEffect(() => {
    if (!view) return;
    setDraft(view.settings.modsDir);
    setDraftState(view.modsDirState);
  }, [view]);

  const runDetection = useCallback(async () => {
    setDetecting(true);
    try {
      const results = await api.settings.detectModsDirs();
      setDetected(results);
    } catch (error) {
      toast.error("Detection failed", toUserMessage(error));
    } finally {
      setDetecting(false);
    }
  }, []);

  // First run convenience: look for a likely install automatically.
  useEffect(() => {
    if (!view || autoDetectAttempted.current) return;
    autoDetectAttempted.current = true;
    if (!view.settings.modsDir) {
      void runDetection();
    }
  }, [view, runDetection]);

  // Debounced validation for the edited path. Unchanged values reuse the state
  // returned with the persisted settings, avoiding a needless round trip.
  useEffect(() => {
    if (!draft) {
      setDraftState(null);
      return;
    }

    const saved = view?.settings.modsDir ?? null;
    if (draft.trim() === (saved ?? "")) {
      setDraftState(view?.modsDirState ?? null);
      return;
    }

    let cancelled = false;
    const timer = setTimeout(() => {
      setValidating(true);
      api.settings
        .validateModsDir(draft)
        .then((state) => {
          if (!cancelled) setDraftState(state);
        })
        .catch((error: unknown) => {
          if (!cancelled) {
            toast.error("Could not check folder", toUserMessage(error));
          }
        })
        .finally(() => {
          if (!cancelled) setValidating(false);
        });
    }, 300);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [draft, view]);

  const handleBrowse = async () => {
    try {
      const picked = await api.settings.chooseModsFolder();
      if (picked) setDraft(picked);
    } catch (error) {
      toast.error("Folder picker failed", toUserMessage(error));
    }
  };

  const handleSave = async () => {
    const value = draft?.trim() ?? "";
    if (!value) {
      toast.error(
        "No folder selected",
        "Choose the Mods folder used by ZZMI before saving.",
      );
      return;
    }

    const saved = await save({
      ...(view?.settings ?? { schemaVersion: 1, modsDir: null }),
      modsDir: value,
    });
    if (saved) {
      toast.success("Settings saved", "ZZMM is linked to your Mods folder.");
    }
  };

  const dirty = (draft?.trim() ?? null) !== (view?.settings.modsDir ?? null);

  return (
    <div className="mx-auto w-full max-w-4xl px-8 py-8">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="eyebrow">Sector 06 // System</p>
          <h1 className="mt-1.5 font-display text-[26px] font-semibold tracking-tight text-zinc-50">
            Settings
          </h1>
          <p className="mt-1.5 max-w-xl text-[13px] leading-relaxed text-zinc-500">
            Link ZZMM to your ZZMI folder, review detected installs, and keep an
            eye on local storage. The mod loader does the rest.
          </p>
        </div>
        <Badge tone="muted">Local only</Badge>
      </header>

      {isLoading && !view ? (
        <LoadingState />
      ) : !view && loadError ? (
        <ErrorState message={loadError} onRetry={() => void load()} />
      ) : view ? (
        <div className="mt-8 space-y-5">
          <ModsFolderSection
            draft={draft}
            saved={view.settings.modsDir}
            dirty={dirty}
            state={draftState}
            validating={validating}
            saving={isSaving}
            detecting={detecting}
            onDraftChange={setDraft}
            onBrowse={() => void handleBrowse()}
            onSave={() => void handleSave()}
            onDetect={() => void runDetection()}
          />

          <DetectedInstallations
            results={detected}
            detecting={detecting}
            current={draft}
            onDetect={() => void runDetection()}
            onUse={setDraft}
          />

          <StorageSection paths={view.paths} />
        </div>
      ) : null}
    </div>
  );
}

function LoadingState() {
  return (
    <Panel className="mt-8">
      <div className="flex items-center gap-3 py-6 text-sm text-zinc-400">
        <Loader2 className="h-4 w-4 animate-spin text-acid-400" aria-hidden />
        Reading local configuration…
      </div>
    </Panel>
  );
}

function ErrorState({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <Panel className="mt-8" eyebrow="Configuration error" title="Could not load settings">
      <div className="flex items-start gap-3 border border-hot-500/30 bg-hot-500/5 p-4">
        <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-hot-400" aria-hidden />
        <div className="min-w-0">
          <p className="text-xs leading-relaxed text-zinc-300">{message}</p>
          <p className="mt-2 text-[11px] leading-relaxed text-zinc-500">
            Check the log folder shown below (once the app reloads), then retry.
          </p>
          <Button size="sm" variant="outline" className="mt-3" onClick={onRetry}>
            Retry
          </Button>
        </div>
      </div>
    </Panel>
  );
}
