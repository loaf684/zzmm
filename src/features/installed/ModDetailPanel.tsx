import { useEffect, useMemo, useState, type ReactNode } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  Box,
  CalendarDays,
  Check,
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  FolderOpen,
  HardDrive,
  Image as ImageIcon,
  Keyboard,
  Loader2,
  Pencil,
  RefreshCw,
  Tag,
  Trash2,
  Undo2,
  User,
  X,
  type LucideIcon,
} from "lucide-react";

import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { IconButton } from "@/components/ui/IconButton";
import { Notice } from "@/components/ui/Notice";
import { SuggestionInput } from "@/components/ui/SuggestionInput";
import { Switch } from "@/components/ui/Switch";
import { KeyCapture } from "@/features/hotkeys/KeyCapture";
import { ModelViewer } from "@/features/preview/ModelViewer";
import { cn } from "@/lib/cn";
import { formatBytes, formatIsoDate } from "@/lib/format";
import { useHotkeysStore } from "@/store/hotkeys";
import { useModsStore } from "@/store/mods";
import { usePreviewStore } from "@/store/preview";
import type { InstalledMod, ModHotkey } from "@/types/mods";
import { describeHotkey } from "./hotkeyLabels";

interface ModDetailPanelProps {
  mod: InstalledMod;
  busy: boolean;
  onClose: () => void;
  onToggle: (enabled: boolean) => void;
  onOpen: () => void;
  onDelete: () => void;
  onRenamed: (baseName: string) => void;
}

type ViewerMode = "image" | "3d";

const HOTKEY_TYPES = ["activate", "hold", "toggle", "cycle"] as const;

function hotkeyKey(hotkey: ModHotkey): string {
  return `${hotkey.file}::${hotkey.section}::${hotkey.line}`;
}

export function ModDetailPanel({
  mod,
  busy,
  onClose,
  onToggle,
  onOpen,
  onDelete,
  onRenamed,
}: ModDetailPanelProps) {
  const details = useModsStore((state) => state.details[mod.folderName]);
  const detailLoading = useModsStore(
    (state) => state.detailLoading === mod.folderName,
  );
  const detailError = useModsStore((state) => state.detailError);
  const hotkeys = useModsStore((state) => state.hotkeys[mod.folderName]);
  const hotkeyLoading = useModsStore(
    (state) => state.hotkeyLoading === mod.folderName,
  );
  const hotkeyError = useModsStore((state) => state.hotkeyError);
  const loadDetails = useModsStore((state) => state.loadDetails);
  const loadHotkeys = useModsStore((state) => state.loadHotkeys);
  const saveHotkeys = useModsStore((state) => state.saveHotkeys);
  const saveNotes = useModsStore((state) => state.saveNotes);
  const renameMod = useModsStore((state) => state.rename);
  const changePreview = useModsStore((state) => state.changePreview);
  const setCharacter = useModsStore((state) => state.setCharacter);
  const openUrl = useModsStore((state) => state.openUrl);
  const allMods = useModsStore((state) => state.result?.mods);

  const conflicts = useHotkeysStore((state) => state.conflicts);
  const isScanningConflicts = useHotkeysStore(
    (state) => state.isScanningConflicts,
  );
  const scanConflicts = useHotkeysStore((state) => state.scanConflicts);

  const preview = usePreviewStore((state) => state.preview);
  const previewFolder = usePreviewStore((state) => state.modFolder);
  const previewLoading = usePreviewStore((state) => state.isLoading);
  const previewError = usePreviewStore((state) => state.error);
  const loadPreview = usePreviewStore((state) => state.load);
  const reloadPreview = usePreviewStore((state) => state.reload);
  const hiddenParts = usePreviewStore((state) => state.hiddenParts);
  const wireframe = usePreviewStore((state) => state.wireframe);
  const togglePart = usePreviewStore((state) => state.togglePart);
  const setWireframe = usePreviewStore((state) => state.setWireframe);

  const [activeImage, setActiveImage] = useState(0);
  const [brokenImages, setBrokenImages] = useState<Record<string, boolean>>({});
  const [notesDraft, setNotesDraft] = useState("");
  const [savingNotes, setSavingNotes] = useState(false);
  const [changingPreview, setChangingPreview] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameDraft, setRenameDraft] = useState("");
  const [editingCharacter, setEditingCharacter] = useState(false);
  const [characterDraft, setCharacterDraft] = useState("");
  const [savingCharacter, setSavingCharacter] = useState(false);
  const [viewerMode, setViewerMode] = useState<ViewerMode>("image");
  const [capturingHotkey, setCapturingHotkey] = useState<string | null>(null);
  const [captureNoModifiers, setCaptureNoModifiers] = useState(false);
  const [hotkeyDrafts, setHotkeyDrafts] = useState<
    Record<string, { binding: string; actionType: string }>
  >({});
  const [savingHotkeys, setSavingHotkeys] = useState(false);
  const [dismissedDetailError, setDismissedDetailError] = useState<string | null>(
    null,
  );
  const [dismissedHotkeyError, setDismissedHotkeyError] = useState<string | null>(
    null,
  );

  const characterOptions = useMemo(() => {
    const seen = new Set<string>();
    for (const item of allMods ?? []) {
      const value = item.character.trim();
      if (value && value !== "Uncategorized") seen.add(value);
    }
    return [...seen].sort((a, b) => a.localeCompare(b));
  }, [allMods]);

  const gameBananaId = mod.metadata?.gameBananaModId ?? null;
  const sourceUrl = gameBananaId
    ? `https://gamebanana.com/mods/${gameBananaId}`
    : null;

  useEffect(() => {
    void loadDetails(mod.folderName);
    void loadHotkeys(mod.folderName);
  }, [mod.folderName, loadDetails, loadHotkeys]);

  useEffect(() => {
    setEditingCharacter(false);
    setCapturingHotkey(null);
    setHotkeyDrafts({});
    setDismissedDetailError(null);
    setDismissedHotkeyError(null);
  }, [mod.folderName]);

  // Conflict checks compare every enabled mod. Only scan once this mod is known
  // to have bindings, so opening an outfit-free mod stays cheap.
  useEffect(() => {
    if (!mod.enabled) return;
    if (!hotkeys || hotkeys.length === 0) return;
    void scanConflicts();
  }, [mod.folderName, mod.enabled, hotkeys, scanConflicts]);

  useEffect(() => {
    setActiveImage(0);
    setBrokenImages({});
  }, [mod.folderName]);

  useEffect(() => {
    setNotesDraft(details?.notes ?? "");
  }, [mod.folderName, details?.notes]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (renameOpen) {
        setRenameOpen(false);
        return;
      }
      onClose();
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, renameOpen]);

  const images = useMemo(() => {
    const local = (details?.previews ?? []).map((path) => convertFileSrc(path));
    if (local.length > 0) return local;

    const remote = mod.metadata?.thumbnailUrl;
    return remote ? [remote] : [];
  }, [details?.previews, mod.metadata?.thumbnailUrl]);

  // Show the render when the mod has no image, otherwise let the user switch.
  useEffect(() => {
    setViewerMode(images.length > 0 ? "image" : "3d");
  }, [mod.folderName, images.length]);

  // Build the mesh only when the 3D tab is actually visible.
  useEffect(() => {
    if (viewerMode !== "3d") return;
    if (previewFolder === mod.folderName) return;
    void loadPreview(mod.folderName);
  }, [viewerMode, mod.folderName, previewFolder, loadPreview]);

  const visibleImage = images[activeImage];
  const showImage = Boolean(visibleImage) && !brokenImages[visibleImage];
  const previewForMod = preview?.modFolder === mod.folderName ? preview : null;
  const previewBusy = previewLoading && previewFolder === mod.folderName;
  const previewFailure =
    previewError && previewFolder === mod.folderName ? previewError : null;

  const hotkeyGroups = useMemo(() => {
    const groups = new Map<string, ModHotkey[]>();
    for (const hotkey of hotkeys ?? []) {
      const list = groups.get(hotkey.file) ?? [];
      list.push(hotkey);
      groups.set(hotkey.file, list);
    }
    return [...groups.entries()];
  }, [hotkeys]);

  const hotkeysByKey = useMemo(() => {
    const map = new Map<string, ModHotkey>();
    for (const hotkey of hotkeys ?? []) map.set(hotkeyKey(hotkey), hotkey);
    return map;
  }, [hotkeys]);

  // Edits live in a draft map so the user can change several bindings before
  // committing them with the single footer action.
  const dirtyDrafts = useMemo(
    () =>
      Object.entries(hotkeyDrafts).filter(([key, draft]) => {
        const hotkey = hotkeysByKey.get(key);
        if (!hotkey) return false;
        return (
          draft.binding !== hotkey.binding ||
          draft.actionType !== hotkey.actionType
        );
      }),
    [hotkeyDrafts, hotkeysByKey],
  );

  const modConflicts = useMemo(() => {
    if (!conflicts || !mod.enabled) return [];

    return conflicts.conflicts
      .map((conflict) => ({
        ...conflict,
        others: conflict.entries.filter(
          (entry) => entry.folderName !== mod.folderName,
        ),
      }))
      .filter(
        (conflict) =>
          conflict.others.length > 0 &&
          conflict.entries.some((entry) => entry.folderName === mod.folderName),
      );
  }, [conflicts, mod.enabled, mod.folderName]);

  const notesDirty = notesDraft.trim() !== (details?.notes ?? "").trim();

  const handleSaveNotes = async () => {
    setSavingNotes(true);
    const saved = await saveNotes(mod.folderName, notesDraft);
    setSavingNotes(false);
    if (saved) setNotesDraft(notesDraft.trim());
  };

  const handleChangePreview = async () => {
    setChangingPreview(true);
    const changed = await changePreview(mod.folderName);
    setChangingPreview(false);

    if (changed) {
      setBrokenImages({});
      setActiveImage(0);
      await loadDetails(mod.folderName, true);
    }
  };

  const submitRename = async () => {
    const nextName = renameDraft.trim();
    if (!nextName || nextName === mod.baseName) {
      setRenameOpen(false);
      return;
    }

    setRenaming(true);
    const renamedTo = await renameMod(mod.folderName, nextName);
    setRenaming(false);

    if (renamedTo) {
      setRenameOpen(false);
      onRenamed(renamedTo);
    }
  };

  const submitCharacter = async () => {
    setSavingCharacter(true);
    const saved = await setCharacter(
      mod.folderName,
      characterDraft.trim() || null,
    );
    setSavingCharacter(false);
    if (saved) setEditingCharacter(false);
  };

  const setHotkeyDraft = (
    hotkey: ModHotkey,
    patch: Partial<{ binding: string; actionType: string }>,
  ) => {
    const key = hotkeyKey(hotkey);
    setHotkeyDrafts((current) => {
      const base = current[key] ?? {
        binding: hotkey.binding,
        actionType: hotkey.actionType,
      };
      return { ...current, [key]: { ...base, ...patch } };
    });
  };

  const discardHotkeyDrafts = () => {
    setHotkeyDrafts({});
    setCapturingHotkey(null);
  };

  const handleSaveHotkeys = async () => {
    if (dirtyDrafts.length === 0) return;

    const edits = dirtyDrafts.flatMap(([key, draft]) => {
      const hotkey = hotkeysByKey.get(key);
      return hotkey
        ? [{ hotkey, binding: draft.binding, actionType: draft.actionType }]
        : [];
    });

    setSavingHotkeys(true);
    const saved = await saveHotkeys(mod.folderName, edits);
    setSavingHotkeys(false);

    if (saved) {
      setHotkeyDrafts({});
      setCapturingHotkey(null);
    }
  };

  return (
    <div
      className="fixed inset-0 z-[55] flex bg-ink-950/70 backdrop-blur-sm"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !renaming) onClose();
      }}
    >
      <aside
        role="dialog"
        aria-modal="true"
        aria-label={`${mod.displayName} details`}
        className="ml-auto flex h-full w-full max-w-[660px] animate-panel-in flex-col border-l border-ink-700/80 bg-gradient-to-b from-ink-950 via-ink-950 to-ink-900/95 shadow-pop"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex shrink-0 items-start justify-between gap-3 border-b border-ink-800/80 px-6 py-5">
          <div className="min-w-0">
            <p className="eyebrow">{mod.character.trim() || "Uncategorized"}</p>
            <h2
              className="mt-1.5 truncate font-display text-xl font-semibold leading-tight text-zinc-50"
              title={mod.displayName}
            >
              {mod.displayName}
            </h2>
            <div className="mt-2 flex flex-wrap items-center gap-1.5">
              <Badge tone={mod.enabled ? "acid" : "muted"}>
                {mod.enabled ? "Enabled" : "Disabled"}
              </Badge>
              {!mod.linked ? <Badge tone="cyan">Unlinked</Badge> : null}
              {mod.hasConflict ? <Badge tone="hot">Hash conflict</Badge> : null}
              {mod.hotkeyCount > 0 ? (
                <Badge tone="acid">
                  <Keyboard className="mr-1 h-2.5 w-2.5" aria-hidden />
                  {mod.hotkeyCount} hotkey{mod.hotkeyCount === 1 ? "" : "s"}
                </Badge>
              ) : null}
            </div>
          </div>

          <IconButton label="Close details" onClick={onClose} className="shrink-0">
            <X className="h-4 w-4" aria-hidden />
          </IconButton>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto">
          <section aria-label="Preview" className="relative">
            <div className="relative aspect-[16/10] w-full overflow-hidden border-b border-ink-800/80 bg-gradient-to-b from-ink-850 to-ink-950">
              {viewerMode === "3d" ? (
                <div className="absolute inset-0">
                  {previewForMod && previewForMod.parts.length > 0 ? (
                    <ModelViewer
                      parts={previewForMod.parts}
                      hiddenParts={hiddenParts}
                      wireframe={wireframe}
                      className="h-full w-full border-0"
                    />
                  ) : previewBusy ? (
                    <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
                      <Loader2
                        className="h-5 w-5 animate-spin text-acid-400"
                        aria-hidden
                      />
                      <p className="text-[11px] text-zinc-500">
                        Decoding mesh buffers…
                      </p>
                    </div>
                  ) : previewFailure ? (
                    <div className="flex h-full flex-col items-center justify-center gap-2 px-8 text-center">
                      <Box className="h-5 w-5 text-zinc-600" aria-hidden />
                      <p className="text-[11px] font-medium text-zinc-400">
                        3D preview unavailable
                      </p>
                      <p className="max-h-16 overflow-y-auto text-[10px] leading-relaxed text-zinc-600">
                        {previewFailure}
                      </p>
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => void reloadPreview()}
                      >
                        Try again
                      </Button>
                    </div>
                  ) : (
                    <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
                      <Box className="h-5 w-5 text-zinc-600" aria-hidden />
                      <p className="text-[11px] text-zinc-500">
                        No mesh buffers found in this mod.
                      </p>
                    </div>
                  )}
                </div>
              ) : showImage ? (
                <img
                  src={visibleImage}
                  alt={`${mod.displayName} preview`}
                  className="h-full w-full object-contain"
                  onError={() =>
                    setBrokenImages((current) => ({
                      ...current,
                      [visibleImage]: true,
                    }))
                  }
                />
              ) : (
                <div className="flex h-full w-full flex-col items-center justify-center gap-2 bg-gradient-to-br from-ink-800 via-ink-850 to-ink-900 text-center">
                  <span className="font-display text-5xl font-bold text-acid-400/25">
                    {(mod.displayName.trim() || mod.baseName)
                      .slice(0, 1)
                      .toUpperCase()}
                  </span>
                  <span className="text-[10px] uppercase tracking-[0.16em] text-zinc-600">
                    No preview image
                  </span>
                </div>
              )}

              <div className="absolute left-3 top-3 z-10 inline-flex rounded-full border border-white/10 bg-ink-950/75 p-0.5 backdrop-blur">
                <ViewerTab
                  active={viewerMode === "image"}
                  onClick={() => setViewerMode("image")}
                >
                  <ImageIcon className="h-3 w-3" aria-hidden />
                  Image
                </ViewerTab>
                <ViewerTab
                  active={viewerMode === "3d"}
                  onClick={() => setViewerMode("3d")}
                >
                  <Box className="h-3 w-3" aria-hidden />
                  3D
                </ViewerTab>
              </div>

              {viewerMode === "3d" && previewForMod ? (
                <div className="absolute right-3 top-3 z-10 flex items-center gap-1 rounded-full border border-white/10 bg-ink-950/75 p-1 backdrop-blur">
                  {previewForMod.togglesApplied ? (
                    <span
                      className="rounded-full bg-acid-400/15 px-2 py-1 font-mono text-[8px] uppercase tracking-[0.12em] text-acid-300"
                      title="Rendered with the in-game toggle state (d3dx_user.ini and INI defaults)"
                    >
                      in-game
                    </span>
                  ) : null}
                  <button
                    type="button"
                    onClick={() => setWireframe(!wireframe)}
                    aria-pressed={wireframe}
                    className={cn(
                      "rounded-full px-2 py-1 text-[9px] font-medium uppercase tracking-[0.12em] transition-colors",
                      wireframe
                        ? "bg-acid-400/15 text-acid-300"
                        : "text-zinc-500 hover:text-zinc-200",
                    )}
                  >
                    Wire
                  </button>
                  <IconButton
                    label="Rebuild 3D preview"
                    onClick={() => void reloadPreview()}
                  >
                    <RefreshCw className="h-3 w-3" aria-hidden />
                  </IconButton>
                </div>
              ) : null}

              {viewerMode === "image" && images.length > 1 ? (
                <>
                  <CarouselButton
                    side="left"
                    label="Previous image"
                    onClick={() =>
                      setActiveImage((current) =>
                        current === 0 ? images.length - 1 : current - 1,
                      )
                    }
                  />
                  <CarouselButton
                    side="right"
                    label="Next image"
                    onClick={() =>
                      setActiveImage((current) =>
                        current === images.length - 1 ? 0 : current + 1,
                      )
                    }
                  />
                  <span className="absolute bottom-2 right-2 rounded-full border border-white/10 bg-ink-950/70 px-2 py-0.5 font-mono text-[10px] text-zinc-300 backdrop-blur">
                    {activeImage + 1} / {images.length}
                  </span>
                </>
              ) : null}
            </div>

            {viewerMode === "image" && images.length > 1 ? (
              <div className="flex gap-2 overflow-x-auto border-b border-ink-800/80 px-6 py-3">
                {images.map((image, index) => (
                  <button
                    key={image}
                    type="button"
                    onClick={() => setActiveImage(index)}
                    aria-label={`Show image ${index + 1}`}
                    aria-pressed={index === activeImage}
                    className={cn(
                      "h-12 w-20 shrink-0 overflow-hidden rounded-lg border transition-colors",
                      index === activeImage
                        ? "border-acid-400"
                        : "border-ink-700 hover:border-ink-500",
                    )}
                  >
                    {brokenImages[image] ? (
                      <span className="flex h-full w-full items-center justify-center bg-ink-850 text-zinc-700">
                        <ImageIcon className="h-4 w-4" aria-hidden />
                      </span>
                    ) : (
                      <img
                        src={image}
                        alt=""
                        loading="lazy"
                        className="h-full w-full object-cover"
                        onError={() =>
                          setBrokenImages((current) => ({
                            ...current,
                            [image]: true,
                          }))
                        }
                      />
                    )}
                  </button>
                ))}
              </div>
            ) : null}

            {viewerMode === "3d" &&
            previewForMod &&
            previewForMod.parts.length > 1 ? (
              <div className="flex flex-wrap gap-1.5 border-b border-ink-800/80 px-6 py-2.5">
                {previewForMod.parts.map((part) => {
                  const visible = !hiddenParts.includes(part.name);
                  return (
                    <button
                      key={part.name}
                      type="button"
                      onClick={() => togglePart(part.name)}
                      aria-pressed={visible}
                      className={cn(
                        "rounded-full border px-2 py-0.5 text-[10px] transition-colors",
                        visible
                          ? "border-acid-400/40 bg-acid-400/10 text-acid-300"
                          : "border-ink-700 text-zinc-600 hover:text-zinc-400",
                      )}
                      title={`${part.triangleCount.toLocaleString()} triangles`}
                    >
                      {part.name}
                    </button>
                  );
                })}
              </div>
            ) : null}
          </section>

          <div className="space-y-6 px-6 py-6">
            <section aria-label="Actions">
              <div className="flex flex-wrap items-center gap-2 rounded-2xl border border-ink-800/80 bg-ink-900/55 p-2">
                <div className="flex min-w-0 flex-1 items-center gap-2.5 px-1.5">
                  <Switch
                    checked={mod.enabled}
                    onCheckedChange={onToggle}
                    disabled={busy}
                    label={`${mod.enabled ? "Disable" : "Enable"} ${mod.displayName}`}
                  />
                  <div className="min-w-0">
                    <p className="text-[12px] font-medium text-zinc-200">
                      {mod.enabled ? "Enabled in ZZMI" : "Disabled in ZZMI"}
                    </p>
                    <p className="truncate text-[10px] text-zinc-500">
                      {mod.enabled
                        ? "The folder loads with the game."
                        : "The DISABLED_ prefix keeps it out of the game."}
                    </p>
                  </div>
                </div>

                <div className="flex flex-wrap items-center justify-end gap-1">
                  <Button variant="ghost" size="sm" onClick={onOpen} disabled={busy}>
                    <FolderOpen className="h-3.5 w-3.5" aria-hidden />
                    Open
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      setRenameDraft(mod.baseName);
                      setRenameOpen(true);
                    }}
                    disabled={busy}
                  >
                    <Pencil className="h-3.5 w-3.5" aria-hidden />
                    Rename
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => void handleChangePreview()}
                    loading={changingPreview}
                    disabled={busy}
                  >
                    <ImageIcon className="h-3.5 w-3.5" aria-hidden />
                    Preview
                  </Button>
                  {sourceUrl ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void openUrl(sourceUrl)}
                    >
                      <ExternalLink className="h-3.5 w-3.5" aria-hidden />
                      GameBanana
                    </Button>
                  ) : null}
                  <IconButton
                    label="Delete mod"
                    tone="danger"
                    onClick={onDelete}
                    disabled={busy}
                    className="ml-1"
                  >
                    <Trash2 className="h-3.5 w-3.5" aria-hidden />
                  </IconButton>
                </div>
              </div>
            </section>

            <section aria-label="Information">
              <SectionHeading
                title="Information"
                action={
                  detailLoading ? (
                    <Loader2
                      className="h-3.5 w-3.5 animate-spin text-acid-400"
                      aria-hidden
                    />
                  ) : undefined
                }
              />

              {detailError && dismissedDetailError !== detailError ? (
                <Notice
                  className="mt-2"
                  message={detailError}
                  onDismiss={() => setDismissedDetailError(detailError)}
                  action={
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => void loadDetails(mod.folderName, true)}
                    >
                      Try again
                    </Button>
                  }
                />
              ) : null}

              <dl className="mt-2 grid grid-cols-2 gap-x-6 gap-y-4 rounded-2xl border border-ink-800/70 bg-ink-900/45 px-4 py-4">
                <div className="min-w-0">
                  <dt className="flex items-center gap-1 text-[9px] uppercase tracking-[0.14em] text-zinc-600">
                    <User className="h-2.5 w-2.5" aria-hidden />
                    Character
                  </dt>
                  <dd className="mt-1">
                    {editingCharacter ? (
                      <form
                        className="flex items-center gap-1"
                        onSubmit={(event) => {
                          event.preventDefault();
                          void submitCharacter();
                        }}
                      >
                        <SuggestionInput
                          value={characterDraft}
                          onChange={setCharacterDraft}
                          suggestions={characterOptions}
                          maxLength={80}
                          placeholder="Character name"
                          ariaLabel="Character name"
                          autoFocus
                          className="min-w-0 flex-1"
                          inputClassName="h-7 rounded-lg border-ink-600 bg-ink-950 px-2 text-[11px]"
                        />
                        <button
                          type="submit"
                          disabled={savingCharacter}
                          aria-label="Save character"
                          className="rounded p-1 text-acid-300 transition-colors hover:text-acid-200 disabled:opacity-50"
                        >
                          {savingCharacter ? (
                            <Loader2 className="h-3 w-3 animate-spin" aria-hidden />
                          ) : (
                            <Check className="h-3 w-3" aria-hidden />
                          )}
                        </button>
                        <button
                          type="button"
                          onClick={() => setEditingCharacter(false)}
                          aria-label="Cancel character edit"
                          className="rounded p-1 text-zinc-500 transition-colors hover:text-zinc-300"
                        >
                          <X className="h-3 w-3" aria-hidden />
                        </button>
                      </form>
                    ) : (
                      <button
                        type="button"
                        onClick={() => {
                          setCharacterDraft(
                            mod.character === "Uncategorized"
                              ? ""
                              : mod.character,
                          );
                          setEditingCharacter(true);
                        }}
                        title="Change character"
                        className="group inline-flex max-w-full items-center gap-1 text-[12px] text-zinc-300 transition-colors hover:text-acid-300"
                      >
                        <span className="truncate">
                          {mod.character || "Uncategorized"}
                        </span>
                        <Pencil
                          className="h-2.5 w-2.5 shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
                          aria-hidden
                        />
                      </button>
                    )}
                  </dd>
                </div>
                <InfoItem
                  icon={User}
                  label="Author"
                  value={mod.metadata?.author?.trim() || "—"}
                />
                <InfoItem
                  icon={Tag}
                  label="Version"
                  value={mod.metadata?.version?.trim() || "—"}
                />
                <InfoItem
                  icon={CalendarDays}
                  label="Installed"
                  value={formatIsoDate(details?.installedAt)}
                />
                <InfoItem
                  icon={HardDrive}
                  label="Folder size"
                  value={details ? formatBytes(details.folderSize) : "…"}
                />
                <InfoItem
                  icon={ExternalLink}
                  label="Source"
                  value={
                    gameBananaId ? `GameBanana #${gameBananaId}` : "Local / unknown"
                  }
                />
                <div className="col-span-2 min-w-0">
                  <dt className="text-[9px] uppercase tracking-[0.14em] text-zinc-600">
                    Folder
                  </dt>
                  <dd
                    className="mt-1 truncate font-mono text-[10px] text-zinc-500"
                    title={mod.folderName}
                  >
                    {mod.folderName}
                  </dd>
                </div>
              </dl>
            </section>

            <section aria-label="Hotkeys">
              <SectionHeading
                title={
                  <span className="flex items-center gap-2">
                    Hotkeys
                    {hotkeys && hotkeys.length > 0 ? (
                      <span className="font-mono text-[10px] normal-case tracking-normal text-zinc-600">
                        {hotkeys.length}
                      </span>
                    ) : null}
                    {modConflicts.length > 0 ? (
                      <Badge tone="hot">Conflict</Badge>
                    ) : null}
                  </span>
                }
                action={
                  <div className="flex items-center gap-3">
                    <label
                      className="flex cursor-pointer items-center gap-1.5 text-[10px] text-zinc-500 transition-colors hover:text-zinc-300"
                      title="Write NO_MODIFIERS so presses with Ctrl/Alt/Shift don't trigger the binding"
                    >
                      <input
                        type="checkbox"
                        checked={captureNoModifiers}
                        onChange={(event) =>
                          setCaptureNoModifiers(event.target.checked)
                        }
                        className="h-3 w-3 rounded accent-acid-400"
                      />
                      No modifiers
                    </label>
                    <IconButton
                      label="Rescan hotkeys"
                      onClick={() => void loadHotkeys(mod.folderName, true)}
                    >
                      <RefreshCw className="h-3 w-3" aria-hidden />
                    </IconButton>
                  </div>
                }
              />

              {hotkeyLoading && !hotkeys ? (
                <div className="mt-3 flex items-center gap-2 text-[11px] text-zinc-500">
                  <Loader2
                    className="h-3.5 w-3.5 animate-spin text-acid-400"
                    aria-hidden
                  />
                  Reading hotkeys…
                </div>
              ) : null}

              {hotkeyError && dismissedHotkeyError !== hotkeyError ? (
                <Notice
                  className="mt-2"
                  message={hotkeyError}
                  onDismiss={() => setDismissedHotkeyError(hotkeyError)}
                  action={
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => void loadHotkeys(mod.folderName, true)}
                    >
                      Try again
                    </Button>
                  }
                />
              ) : null}

              {modConflicts.length > 0 ? (
                <div className="mt-2 rounded-xl border border-hot-500/25 bg-hot-500/[0.04] p-3">
                  <p className="flex items-center gap-1.5 text-[11px] font-medium text-hot-400">
                    <AlertTriangle className="h-3.5 w-3.5" aria-hidden />
                    {modConflicts.length} hotkey conflict
                    {modConflicts.length === 1 ? "" : "s"} with enabled mods
                  </p>
                  <ul className="mt-1.5 space-y-1">
                    {modConflicts.slice(0, 4).map((conflict, index) => (
                      <li
                        key={`${conflict.key}-${conflict.binding}-${index}`}
                        className="text-[10px] leading-relaxed text-zinc-400"
                      >
                        <span className="font-mono text-hot-300">
                          {conflict.binding || conflict.key}
                        </span>
                        <span className="text-zinc-600"> — </span>
                        {[
                          ...new Set(
                            conflict.others.map((entry) => entry.displayName),
                          ),
                        ].join(", ")}
                      </li>
                    ))}
                  </ul>
                </div>
              ) : isScanningConflicts && mod.enabled ? (
                <p className="mt-2 flex items-center gap-1.5 text-[10px] text-zinc-600">
                  <Loader2 className="h-3 w-3 animate-spin" aria-hidden />
                  Checking other enabled mods…
                </p>
              ) : null}

              {!hotkeyLoading && hotkeys && hotkeys.length === 0 ? (
                <p className="mt-2 rounded-xl border border-dashed border-ink-800 px-3 py-3 text-[11px] text-zinc-600">
                  No `[Key…]` bindings found in this mod&apos;s INI files.
                </p>
              ) : null}

              {hotkeyGroups.length > 0 ? (
                <div className="mt-2 overflow-hidden rounded-2xl border border-ink-800/80 bg-ink-900/40">
                  <div className="divide-y divide-ink-800/70">
                    {hotkeyGroups.map(([file, entries]) => (
                      <div key={file} className="px-3.5 py-3">
                        <p
                          className="truncate font-mono text-[10px] text-zinc-600"
                          title={file}
                        >
                          {file}
                        </p>
                        <ul className="mt-2 space-y-1">
                          {entries.map((hotkey) => {
                            const key = hotkeyKey(hotkey);
                            const draft = hotkeyDrafts[key] ?? {
                              binding: hotkey.binding,
                              actionType: hotkey.actionType,
                            };
                            const dirty =
                              draft.binding !== hotkey.binding ||
                              draft.actionType !== hotkey.actionType;
                            return (
                              <li
                                key={key}
                                className={cn(
                                  "group rounded-xl px-2 py-2 transition-colors",
                                  dirty
                                    ? "bg-amber-400/[0.04] ring-1 ring-inset ring-amber-400/20"
                                    : "hover:bg-ink-900/70",
                                )}
                              >
                                <div className="flex items-center gap-2">
                                  <KeyCapture
                                    value={draft.binding}
                                    capturing={capturingHotkey === key}
                                    noModifiers={captureNoModifiers}
                                    disabled={savingHotkeys}
                                    onToggle={() =>
                                      setCapturingHotkey((current) =>
                                        current === key ? null : key,
                                      )
                                    }
                                    onCommit={(binding) => {
                                      setHotkeyDraft(hotkey, { binding });
                                      setCapturingHotkey(null);
                                    }}
                                  />
                                  <span aria-hidden className="text-zinc-700">
                                    →
                                  </span>
                                  <span
                                    className="min-w-0 flex-1 truncate text-[11px] text-zinc-300"
                                    title={describeHotkey(
                                      hotkey.section,
                                      hotkey.description,
                                    )}
                                  >
                                    {describeHotkey(
                                      hotkey.section,
                                      hotkey.description,
                                    )}
                                  </span>
                                  {hotkey.options > 1 ? (
                                    <span className="shrink-0 font-mono text-[9px] text-zinc-600">
                                      {hotkey.options} options
                                    </span>
                                  ) : null}
                                  {dirty ? (
                                    <IconButton
                                      label={`Discard change to ${hotkey.section}`}
                                      onClick={() =>
                                        setHotkeyDrafts((current) => {
                                          const next = { ...current };
                                          delete next[key];
                                          return next;
                                        })
                                      }
                                    >
                                      <Undo2 className="h-3 w-3" aria-hidden />
                                    </IconButton>
                                  ) : null}
                                </div>
                                <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 pl-2 font-mono text-[9px] text-zinc-600">
                                  <span>[{hotkey.section}]</span>
                                  <label
                                    className="inline-flex items-center gap-1"
                                    title="How the key press is interpreted"
                                  >
                                    <span className="sr-only">Action type</span>
                                    <select
                                      value={draft.actionType}
                                      disabled={savingHotkeys}
                                      onChange={(event) =>
                                        setHotkeyDraft(hotkey, {
                                          actionType: event.target.value,
                                        })
                                      }
                                      className="h-5 rounded border border-ink-700 bg-ink-950/80 px-1 font-mono text-[9px] uppercase tracking-[0.1em] text-zinc-400 transition-colors hover:border-ink-600 focus:border-acid-400/60 focus:outline-none disabled:opacity-50"
                                    >
                                      {HOTKEY_TYPES.map((type) => (
                                        <option key={type} value={type}>
                                          {type}
                                        </option>
                                      ))}
                                      {draft.actionType === "custom" ? (
                                        <option value="custom" disabled>
                                          custom
                                        </option>
                                      ) : null}
                                    </select>
                                  </label>
                                  {dirty ? (
                                    <span className="rounded border border-amber-400/25 px-1 py-0.5 uppercase tracking-[0.1em] text-amber-300/80">
                                      pending
                                    </span>
                                  ) : null}
                                  {hotkey.condition ? (
                                    <span
                                      className="min-w-0 truncate text-cyan-300/60"
                                      title={hotkey.condition}
                                    >
                                      if {hotkey.condition}
                                    </span>
                                  ) : null}
                                </div>
                              </li>
                            );
                          })}
                        </ul>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}
            </section>

            <section aria-label="Notes">
              <SectionHeading
                title="Notes"
                action={
                  notesDirty ? (
                    <span className="text-[10px] text-amber-300/80">Unsaved</span>
                  ) : undefined
                }
              />
              <textarea
                value={notesDraft}
                onChange={(event) => setNotesDraft(event.target.value)}
                rows={4}
                placeholder="Add your own notes about this mod…"
                className="mt-2 w-full resize-y rounded-xl border border-ink-800 bg-ink-900/50 px-3.5 py-2.5 text-[12px] leading-relaxed text-zinc-200 placeholder:text-zinc-600 focus:border-ink-600 focus:outline-none"
              />
              <div className="mt-2 flex justify-end">
                <Button
                  size="sm"
                  variant="primary"
                  onClick={() => void handleSaveNotes()}
                  loading={savingNotes}
                  disabled={!notesDirty}
                >
                  Save notes
                </Button>
              </div>
            </section>
          </div>
        </div>

        {dirtyDrafts.length > 0 ? (
          <footer className="flex shrink-0 items-center justify-between gap-3 border-t border-amber-400/20 bg-ink-950/95 px-6 py-3 shadow-[0_-12px_30px_-24px_rgba(0,0,0,0.9)] backdrop-blur">
            <p className="min-w-0 text-[11px] text-zinc-400">
              <span className="font-mono text-amber-300">
                {dirtyDrafts.length}
              </span>{" "}
              unsaved hotkey{dirtyDrafts.length === 1 ? "" : "s"}
              <span className="ml-1.5 hidden text-zinc-600 sm:inline">
                · edits stay local until you save
              </span>
            </p>
            <div className="flex shrink-0 items-center gap-2">
              <Button
                variant="ghost"
                size="sm"
                onClick={discardHotkeyDrafts}
                disabled={savingHotkeys}
              >
                Discard
              </Button>
              <Button
                variant="primary"
                size="sm"
                onClick={() => void handleSaveHotkeys()}
                loading={savingHotkeys}
              >
                Save all
              </Button>
            </div>
          </footer>
        ) : null}
      </aside>

      {renameOpen ? (
        <div
          className="fixed inset-0 z-[70] flex items-center justify-center bg-ink-950/70 p-6 backdrop-blur-sm"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget && !renaming) {
              setRenameOpen(false);
            }
          }}
        >
          <form
            className="w-full max-w-md animate-fade-in rounded-2xl border border-ink-700 bg-ink-900 p-5 shadow-pop"
            onSubmit={(event) => {
              event.preventDefault();
              void submitRename();
            }}
          >
            <h3 className="font-display text-[15px] font-semibold text-zinc-100">
              Rename mod folder
            </h3>
            <p className="mt-1.5 text-[11px] leading-relaxed text-zinc-500">
              Only the folder name changes. The current enabled or disabled state
              is preserved.
            </p>
            <input
              value={renameDraft}
              onChange={(event) => setRenameDraft(event.target.value)}
              autoFocus
              disabled={renaming}
              aria-label="New mod folder name"
              className="mt-4 h-9 w-full rounded-lg border border-ink-700 bg-ink-950/60 px-3 text-[12px] text-zinc-200 focus:border-acid-400/60 focus:outline-none"
            />
            <div className="mt-4 flex justify-end gap-2">
              <Button
                variant="ghost"
                onClick={() => setRenameOpen(false)}
                disabled={renaming}
              >
                Cancel
              </Button>
              <Button
                type="submit"
                variant="primary"
                loading={renaming}
                disabled={!renameDraft.trim()}
              >
                Rename
              </Button>
            </div>
          </form>
        </div>
      ) : null}
    </div>
  );
}

function ViewerTab({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={cn(
        "inline-flex h-7 items-center gap-1.5 rounded-full px-2.5 text-[10px] font-medium transition-colors",
        active
          ? "bg-zinc-100 text-ink-950"
          : "text-zinc-400 hover:text-zinc-100",
      )}
    >
      {children}
    </button>
  );
}

function SectionHeading({
  title,
  action,
}: {
  title: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-2">
      <h3 className="flex items-center gap-2 text-[11px] font-medium uppercase tracking-[0.14em] text-zinc-500">
        {title}
      </h3>
      {action}
    </div>
  );
}

function InfoItem({
  icon: Icon,
  label,
  value,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
}) {
  return (
    <div className="min-w-0">
      <dt className="flex items-center gap-1 text-[9px] uppercase tracking-[0.14em] text-zinc-600">
        <Icon className="h-2.5 w-2.5" aria-hidden />
        {label}
      </dt>
      <dd className="mt-1 truncate text-[12px] text-zinc-300" title={value}>
        {value}
      </dd>
    </div>
  );
}

function CarouselButton({
  side,
  label,
  onClick,
}: {
  side: "left" | "right";
  label: string;
  onClick: () => void;
}) {
  const Icon = side === "left" ? ChevronLeft : ChevronRight;

  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      className={cn(
        "absolute top-1/2 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-full border border-white/10 bg-ink-950/70 text-zinc-200 backdrop-blur transition-colors hover:bg-ink-900",
        side === "left" ? "left-2" : "right-2",
      )}
    >
      <Icon className="h-4 w-4" aria-hidden />
    </button>
  );
}
