import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  AlertTriangle,
  ArchiveRestore,
  ArrowDownToLine,
  FolderOpen,
  Gamepad2,
  Keyboard,
  LayoutGrid,
  Loader2,
  RefreshCw,
  Rows3,
  Users,
} from "lucide-react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { Button } from "@/components/ui/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { Notice } from "@/components/ui/Notice";
import { Panel } from "@/components/ui/Panel";
import { cn } from "@/lib/cn";
import { useModsStore } from "@/store/mods";
import { useSettingsStore } from "@/store/settings";
import type { InstalledMod, ModScanProgress } from "@/types/mods";
import { HashConflicts } from "./HashConflicts";
import { LoadoutsPanel } from "./LoadoutsPanel";
import { ModCard, ModRow } from "./ModCard";
import { ModDetailPanel } from "./ModDetailPanel";
import { UpdatesPanel } from "./UpdatesPanel";

interface CharacterGroup {
  character: string;
  mods: InstalledMod[];
}

type ViewMode = "grid" | "list";
type ModFilter = "all" | "enabled" | "disabled";
type ToolTab = "conflicts" | "updates" | "loadouts";

const VIEW_KEY = "zzmm.installed.view";
const SIZE_KEY = "zzmm.installed.cardSize";
const GROUP_KEY = "zzmm.installed.group";
const MIN_CARD_SIZE = 190;
const MAX_CARD_SIZE = 360;

function readViewMode(): ViewMode {
  try {
    return localStorage.getItem(VIEW_KEY) === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
}

function readCardSize(): number {
  try {
    const value = Number(localStorage.getItem(SIZE_KEY));
    return Number.isFinite(value) && value >= MIN_CARD_SIZE && value <= MAX_CARD_SIZE
      ? value
      : 230;
  } catch {
    return 230;
  }
}

function readGroupByCharacter(): boolean {
  try {
    return localStorage.getItem(GROUP_KEY) === "1";
  } catch {
    return false;
  }
}

function groupMods(mods: InstalledMod[]): CharacterGroup[] {
  const groups = new Map<string, InstalledMod[]>();

  for (const mod of mods) {
    const character = mod.character.trim() || "Uncategorized";
    const list = groups.get(character) ?? [];
    list.push(mod);
    groups.set(character, list);
  }

  return [...groups.entries()]
    .map(([character, entries]) => ({
      character,
      mods: [...entries].sort((a, b) =>
        a.displayName.localeCompare(b.displayName),
      ),
    }))
    .sort((a, b) => {
      if (a.character === "Uncategorized") return 1;
      if (b.character === "Uncategorized") return -1;
      return a.character.localeCompare(b.character);
    });
}

function samePath(a: string | null, b: string | null): boolean {
  if (!a || !b) return false;
  const normalize = (value: string) => value.replace(/[\\/]+$/, "").toLowerCase();
  return normalize(a) === normalize(b);
}

export function InstalledPage() {
  const view = useSettingsStore((state) => state.view);

  const result = useModsStore((state) => state.result);
  const isScanning = useModsStore((state) => state.isScanning);
  const progress = useModsStore((state) => state.progress);
  const scanError = useModsStore((state) => state.scanError);
  const busyFolder = useModsStore((state) => state.busyFolder);
  const scan = useModsStore((state) => state.scan);
  const setModEnabled = useModsStore((state) => state.setEnabled);
  const removeMod = useModsStore((state) => state.remove);
  const openModFolder = useModsStore((state) => state.openFolder);

  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const [pendingDelete, setPendingDelete] = useState<InstalledMod | null>(null);
  const [selectedBaseName, setSelectedBaseName] = useState<string | null>(null);
  const [dismissedScanError, setDismissedScanError] = useState<string | null>(
    null,
  );
  const [characterFilter, setCharacterFilter] = useState(
    searchParams.get("character") ?? "all",
  );
  const [hotkeysOnly, setHotkeysOnly] = useState(false);
  const [conflictsOnly, setConflictsOnly] = useState(false);
  const [viewMode, setViewMode] = useState<ViewMode>(readViewMode);
  const [cardSize, setCardSize] = useState<number>(readCardSize);
  const [groupByCharacter, setGroupByCharacter] = useState<boolean>(
    readGroupByCharacter,
  );
  const [tool, setTool] = useState<ToolTab | null>(null);

  // The sidebar owns the primary library filters. Reading them from the URL
  // keeps the sidebar, deep links, and this page in sync.
  const query = searchParams.get("q") ?? "";
  const stateParam = searchParams.get("state");
  const filter: ModFilter =
    stateParam === "enabled" || stateParam === "disabled" ? stateParam : "all";

  const chooseCharacter = useCallback(
    (value: string) => {
      setCharacterFilter(value);
      const next = new URLSearchParams(searchParams);
      if (value === "all") next.delete("character");
      else next.set("character", value);
      setSearchParams(next, { replace: true });
    },
    [searchParams, setSearchParams],
  );

  // Sidebar character links deep-link into this filter.
  useEffect(() => {
    setCharacterFilter(searchParams.get("character") ?? "all");
  }, [searchParams]);

  const modsDir = view?.settings.modsDir ?? null;
  const folderValid = view?.modsDirState.valid ?? false;
  const currentResult =
    result && samePath(result.modsDir, modsDir) ? result : null;
  const allMods = currentResult?.mods ?? [];

  useEffect(() => {
    try {
      localStorage.setItem(VIEW_KEY, viewMode);
    } catch {
      // Private browsing / storage disabled: keep the in-memory preference.
    }
  }, [viewMode]);

  useEffect(() => {
    try {
      localStorage.setItem(SIZE_KEY, String(cardSize));
    } catch {
      // Private browsing / storage disabled: keep the in-memory preference.
    }
  }, [cardSize]);

  useEffect(() => {
    try {
      localStorage.setItem(GROUP_KEY, groupByCharacter ? "1" : "0");
    } catch {
      // Private browsing / storage disabled: keep the in-memory preference.
    }
  }, [groupByCharacter]);

  const filteredMods = useMemo(() => {
    const needle = query.trim().toLowerCase();

    return allMods.filter((mod) => {
      if (filter === "enabled" && !mod.enabled) return false;
      if (filter === "disabled" && mod.enabled) return false;
      if (
        characterFilter !== "all" &&
        (mod.character.trim() || "Uncategorized") !== characterFilter
      ) {
        return false;
      }
      if (hotkeysOnly && mod.hotkeyCount === 0) return false;
      if (conflictsOnly && !mod.hasConflict) return false;
      if (!needle) return true;
      return (
        mod.displayName.toLowerCase().includes(needle) ||
        mod.folderName.toLowerCase().includes(needle) ||
        mod.character.toLowerCase().includes(needle)
      );
    });
  }, [
    allMods,
    characterFilter,
    conflictsOnly,
    filter,
    hotkeysOnly,
    query,
  ]);

  const characters = useMemo(() => {
    const seen = new Set<string>();
    for (const mod of allMods) {
      seen.add(mod.character.trim() || "Uncategorized");
    }

    return [...seen].sort((a, b) => {
      if (a === "Uncategorized") return 1;
      if (b === "Uncategorized") return -1;
      return a.localeCompare(b);
    });
  }, [allMods]);

  const hotkeyMods = useMemo(
    () => allMods.filter((mod) => mod.hotkeyCount > 0).length,
    [allMods],
  );
  const conflictMods = useMemo(
    () => allMods.filter((mod) => mod.hasConflict).length,
    [allMods],
  );

  const groups = useMemo(() => groupMods(filteredMods), [filteredMods]);

  // Flat view still keeps mods of the same character next to each other, but
  // without the per-character section headers.
  const flatMods = useMemo(
    () =>
      [...filteredMods].sort((a, b) => {
        const aCharacter = a.character.trim() || "Uncategorized";
        const bCharacter = b.character.trim() || "Uncategorized";
        if (aCharacter !== bCharacter) {
          if (aCharacter === "Uncategorized") return 1;
          if (bCharacter === "Uncategorized") return -1;
          return aCharacter.localeCompare(bCharacter);
        }
        return a.displayName.localeCompare(b.displayName);
      }),
    [filteredMods],
  );

  // A character override or rescan can remove the selected group.
  useEffect(() => {
    if (allMods.length === 0 || characterFilter === "all") return;
    if (!characters.includes(characterFilter)) chooseCharacter("all");
  }, [allMods.length, characterFilter, characters, chooseCharacter]);

  const hasActiveFilters =
    query.trim().length > 0 ||
    filter !== "all" ||
    characterFilter !== "all" ||
    hotkeysOnly ||
    conflictsOnly;

  const clearFilters = () => {
    const next = new URLSearchParams(searchParams);
    next.delete("q");
    next.delete("state");
    next.delete("character");
    setSearchParams(next, { replace: true });
    setHotkeysOnly(false);
    setConflictsOnly(false);
  };

  // The base name is stable across enable/disable renames, so the panel stays
  // open when the DISABLED_ prefix is added or removed.
  const selectedMod = selectedBaseName
    ? (allMods.find((mod) => mod.baseName === selectedBaseName) ?? null)
    : null;

  useEffect(() => {
    if (!selectedBaseName) return;
    if (!allMods.some((mod) => mod.baseName === selectedBaseName)) {
      setSelectedBaseName(null);
    }
  }, [allMods, selectedBaseName]);

  useEffect(() => {
    if (modsDir && folderValid) void scan();
  }, [modsDir, folderValid, scan]);

  // Conflicts stay behind the toolbar tab; the count is enough to make them
  // discoverable without pushing the library down on every visit.
  const conflictCount = currentResult?.conflicts.length ?? 0;

  return (
    <div className="mx-auto w-full max-w-[1500px] px-8 py-8">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
            <h1 className="font-display text-[21px] font-semibold tracking-tight text-zinc-50">
              Installed
            </h1>
            <p className="text-[12px] text-zinc-500">
              {currentResult
                ? hasActiveFilters
                  ? `${filteredMods.length} of ${allMods.length} mods · ${currentResult.enabledCount} enabled · ${currentResult.disabledCount} disabled`
                  : `${allMods.length} ${allMods.length === 1 ? "mod" : "mods"} · ${currentResult.enabledCount} enabled · ${currentResult.disabledCount} disabled`
                : "Manage the mods ZZMI loads from your Mods folder."}
            </p>
          </div>
          {hasActiveFilters ? (
            <p className="mt-1 text-[11px] text-zinc-600">
              Filtered by{" "}
              {[
                query.trim() ? `“${query.trim()}”` : null,
                filter !== "all" ? filter : null,
                characterFilter !== "all" ? characterFilter : null,
                hotkeysOnly ? "hotkeys" : null,
                conflictsOnly ? "conflicts" : null,
              ]
                .filter(Boolean)
                .join(" · ")}
            </p>
          ) : null}
        </div>

        <div className="flex shrink-0 gap-1.5">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void openModFolder()}
            disabled={!folderValid}
          >
            <FolderOpen className="h-3.5 w-3.5" aria-hidden />
            Open folder
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void scan()}
            loading={isScanning}
            disabled={!folderValid}
          >
            <RefreshCw className="h-3.5 w-3.5" aria-hidden />
            Rescan
          </Button>
        </div>
      </header>

      {!view ? (
        <Panel className="mt-6">
          <div className="flex items-center gap-3 py-4 text-[12px] text-zinc-400">
            <Loader2 className="h-4 w-4 animate-spin text-acid-400" aria-hidden />
            Reading local configuration…
          </div>
        </Panel>
      ) : !modsDir || !folderValid ? (
        <Panel
          className="mt-6"
          eyebrow="Not linked"
          title={modsDir ? "Mods folder unavailable" : "No Mods folder configured"}
        >
          <div className="flex flex-col items-start gap-3">
            <p className="max-w-xl text-[12px] leading-relaxed text-zinc-500">
              {modsDir
                ? view.modsDirState.message ??
                  "The configured Mods folder cannot be read."
                : "ZZMM reads the folder ZZMI loads mods from. Link it once and this page will fill in automatically."}
            </p>
            <Button variant="primary" onClick={() => navigate("/settings")}>
              Open settings
            </Button>
          </div>
        </Panel>
      ) : (
        <>
          <div className="sticky top-0 z-20 -mx-8 mt-4 border-b border-ink-800/70 bg-ink-950/90 px-8 py-2 backdrop-blur">
            <div className="flex flex-wrap items-center gap-1.5">
              <FilterChip
                active={hotkeysOnly}
                label="Hotkeys"
                count={hotkeyMods}
                onClick={() => setHotkeysOnly((current) => !current)}
              >
                <Keyboard className="h-3.5 w-3.5" aria-hidden />
              </FilterChip>

              <FilterChip
                active={conflictsOnly}
                label="Conflicts"
                count={conflictMods}
                onClick={() => setConflictsOnly((current) => !current)}
                tone={conflictMods > 0 ? "hot" : "default"}
              >
                <AlertTriangle className="h-3.5 w-3.5" aria-hidden />
              </FilterChip>

              {hasActiveFilters ? (
                <button
                  type="button"
                  onClick={clearFilters}
                  className="h-8 rounded-lg px-2 text-[11px] text-zinc-500 transition-colors hover:bg-ink-900 hover:text-zinc-300"
                >
                  Reset
                </button>
              ) : null}

              <span
                aria-hidden
                className="mx-1 hidden h-4 w-px bg-ink-800 sm:block"
              />

              <ToolTabButton
                active={tool === "updates"}
                onClick={() =>
                  setTool((current) =>
                    current === "updates" ? null : "updates",
                  )
                }
              >
                <ArrowDownToLine className="h-3.5 w-3.5" aria-hidden />
                Updates
              </ToolTabButton>
              <ToolTabButton
                active={tool === "loadouts"}
                onClick={() =>
                  setTool((current) =>
                    current === "loadouts" ? null : "loadouts",
                  )
                }
              >
                <ArchiveRestore className="h-3.5 w-3.5" aria-hidden />
                Loadouts
              </ToolTabButton>
              {conflictCount > 0 ? (
                <ToolTabButton
                  active={tool === "conflicts"}
                  tone="hot"
                  onClick={() =>
                    setTool((current) =>
                      current === "conflicts" ? null : "conflicts",
                    )
                  }
                >
                  <AlertTriangle className="h-3.5 w-3.5" aria-hidden />
                  Conflicts
                  <span className="font-mono text-[10px] text-hot-400">
                    {conflictCount}
                  </span>
                </ToolTabButton>
              ) : null}

              <div className="ml-auto flex items-center gap-1.5 pl-2">
                <button
                  type="button"
                  onClick={() => setGroupByCharacter((current) => !current)}
                  aria-pressed={groupByCharacter}
                  title="Group by character"
                  className={cn(
                    "inline-flex h-8 items-center gap-1.5 rounded-lg border px-2.5 text-[11px] font-medium transition-colors duration-150",
                    groupByCharacter
                      ? "border-acid-400/50 bg-acid-400/10 text-acid-300"
                      : "border-ink-700 bg-ink-900 text-zinc-500 hover:border-ink-500 hover:text-zinc-300",
                  )}
                >
                  <Users className="h-3.5 w-3.5" aria-hidden />
                  <span className="hidden lg:inline">Group</span>
                </button>

                <label
                  className={cn(
                    "hidden items-center gap-2 md:flex",
                    viewMode === "list" && "opacity-40",
                  )}
                >
                  <span className="text-[10px] uppercase tracking-[0.14em] text-zinc-600">
                    Size
                  </span>
                  <input
                    type="range"
                    min={MIN_CARD_SIZE}
                    max={MAX_CARD_SIZE}
                    step={20}
                    value={cardSize}
                    onChange={(event) => setCardSize(Number(event.target.value))}
                    disabled={viewMode === "list"}
                    aria-label="Card size"
                    className="size-slider w-24 cursor-pointer"
                  />
                </label>

                <div className="flex items-center rounded-lg border border-ink-700 bg-ink-900 p-0.5">
                  <ViewButton
                    active={viewMode === "grid"}
                    label="Grid view"
                    onClick={() => setViewMode("grid")}
                  >
                    <LayoutGrid className="h-3.5 w-3.5" aria-hidden />
                  </ViewButton>
                  <ViewButton
                    active={viewMode === "list"}
                    label="List view"
                    onClick={() => setViewMode("list")}
                  >
                    <Rows3 className="h-3.5 w-3.5" aria-hidden />
                  </ViewButton>
                </div>
              </div>
            </div>

            {isScanning ? <ScanProgressBar progress={progress} /> : null}
          </div>

          {scanError && dismissedScanError !== scanError ? (
            <Notice
              className="mt-5"
              title="Could not read the Mods folder"
              message={scanError}
              onDismiss={() => setDismissedScanError(scanError)}
              action={
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => void scan()}
                >
                  Try again
                </Button>
              }
            />
          ) : null}

          {currentResult ? (
            <>
              {/* Kept mounted so the one-per-session update check still runs. */}
              <div className={cn("mt-3", tool === "updates" ? "" : "hidden")}>
                <UpdatesPanel enabled={folderValid} />
              </div>
              <div className={cn("mt-3", tool === "loadouts" ? "" : "hidden")}>
                <LoadoutsPanel enabled={folderValid} />
              </div>
              {conflictCount > 0 ? (
                <div
                  className={cn(
                    "mt-3",
                    tool === "conflicts" ? "" : "hidden",
                  )}
                >
                  <HashConflicts conflicts={currentResult.conflicts} />
                </div>
              ) : null}

              {allMods.length === 0 ? (
                <Panel className="mt-6" eyebrow="Empty" title="No mods found yet">
                  <div className="flex flex-col items-center gap-3 py-8 text-center">
                    <Gamepad2 className="h-8 w-8 text-zinc-700" aria-hidden />
                    <p className="max-w-md text-[12px] leading-relaxed text-zinc-500">
                      This folder has no mod subfolders. Drag a .zip, .7z, or
                      .rar archive anywhere into the window to install one, or
                      drop a pack containing several mods at once.
                    </p>
                    <div className="flex gap-2">
                      <Button onClick={() => void openModFolder()}>
                        <FolderOpen className="h-3.5 w-3.5" aria-hidden />
                        Open Mods folder
                      </Button>
                      <Button
                        variant="primary"
                        onClick={() => void scan()}
                        loading={isScanning}
                      >
                        Rescan
                      </Button>
                    </div>
                  </div>
                </Panel>
              ) : filteredMods.length === 0 ? (
                <Panel
                  className="mt-6"
                  eyebrow="No matches"
                  title="Nothing matches your filters"
                  action={
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={clearFilters}
                    >
                      Clear filters
                    </Button>
                  }
                >
                  <p className="text-[12px] leading-relaxed text-zinc-500">
                    Try a different name, character, or enabled state.
                  </p>
                </Panel>
              ) : (
                <div className="mt-4 pb-10">
                  {groupByCharacter ? (
                    <div className="space-y-5">
                      {groups.map((group) => (
                        <section key={group.character}>
                          <div className="mb-2 flex items-center gap-2">
                            <span
                              aria-hidden
                              className="flex h-5 w-5 items-center justify-center rounded-full border border-ink-700 bg-ink-900 font-mono text-[9px] text-acid-300"
                            >
                              {group.character.slice(0, 1).toUpperCase()}
                            </span>
                            <h2 className="text-[11px] font-medium uppercase tracking-[0.14em] text-zinc-400">
                              {group.character}
                            </h2>
                            <span className="font-mono text-[10px] text-zinc-600">
                              {group.mods.length}
                            </span>
                            <span
                              aria-hidden
                              className="h-px flex-1 bg-ink-800/70"
                            />
                          </div>

                          {viewMode === "grid" ? (
                            <div
                              className="grid gap-3"
                              style={{
                                gridTemplateColumns: `repeat(auto-fill, minmax(${cardSize}px, 1fr))`,
                              }}
                            >
                              {group.mods.map((mod) => (
                                <ModCard
                                  key={mod.folderName}
                                  mod={mod}
                                  busy={busyFolder === mod.folderName}
                                  selected={selectedBaseName === mod.baseName}
                                  onSelect={() =>
                                    setSelectedBaseName(mod.baseName)
                                  }
                                  onToggle={(enabled) =>
                                    void setModEnabled(mod.folderName, enabled)
                                  }
                                  onOpen={() =>
                                    void openModFolder(mod.folderName)
                                  }
                                  onDelete={() => setPendingDelete(mod)}
                                />
                              ))}
                            </div>
                          ) : (
                            <div className="flex flex-col gap-1.5">
                              {group.mods.map((mod) => (
                                <ModRow
                                  key={mod.folderName}
                                  mod={mod}
                                  busy={busyFolder === mod.folderName}
                                  selected={selectedBaseName === mod.baseName}
                                  onSelect={() =>
                                    setSelectedBaseName(mod.baseName)
                                  }
                                  onToggle={(enabled) =>
                                    void setModEnabled(mod.folderName, enabled)
                                  }
                                  onOpen={() =>
                                    void openModFolder(mod.folderName)
                                  }
                                  onDelete={() => setPendingDelete(mod)}
                                />
                              ))}
                            </div>
                          )}
                        </section>
                      ))}
                    </div>
                  ) : viewMode === "grid" ? (
                    <div
                      className="grid gap-3"
                      style={{
                        gridTemplateColumns: `repeat(auto-fill, minmax(${cardSize}px, 1fr))`,
                      }}
                    >
                      {flatMods.map((mod) => (
                        <ModCard
                          key={mod.folderName}
                          mod={mod}
                          busy={busyFolder === mod.folderName}
                          selected={selectedBaseName === mod.baseName}
                          onSelect={() => setSelectedBaseName(mod.baseName)}
                          onToggle={(enabled) =>
                            void setModEnabled(mod.folderName, enabled)
                          }
                          onOpen={() => void openModFolder(mod.folderName)}
                          onDelete={() => setPendingDelete(mod)}
                        />
                      ))}
                    </div>
                  ) : (
                    <div className="flex flex-col gap-1.5">
                      {flatMods.map((mod) => (
                        <ModRow
                          key={mod.folderName}
                          mod={mod}
                          busy={busyFolder === mod.folderName}
                          selected={selectedBaseName === mod.baseName}
                          onSelect={() => setSelectedBaseName(mod.baseName)}
                          onToggle={(enabled) =>
                            void setModEnabled(mod.folderName, enabled)
                          }
                          onOpen={() => void openModFolder(mod.folderName)}
                          onDelete={() => setPendingDelete(mod)}
                        />
                      ))}
                    </div>
                  )}

                  {hasActiveFilters ? (
                    <p className="mt-5 text-center text-[11px] text-zinc-600">
                      Showing {filteredMods.length} of {allMods.length} mods
                    </p>
                  ) : null}
                </div>
              )}
            </>
          ) : !isScanning && !scanError ? (
            <Panel className="mt-6" title="No scan yet">
              <Button onClick={() => void scan()}>Scan Mods folder</Button>
            </Panel>
          ) : null}
        </>
      )}

      {selectedMod ? (
        <ModDetailPanel
          mod={selectedMod}
          busy={busyFolder === selectedMod.folderName}
          onClose={() => setSelectedBaseName(null)}
          onToggle={(enabled) =>
            void setModEnabled(selectedMod.folderName, enabled)
          }
          onOpen={() => void openModFolder(selectedMod.folderName)}
          onDelete={() => setPendingDelete(selectedMod)}
          onRenamed={(baseName) => setSelectedBaseName(baseName)}
        />
      ) : null}

      <ConfirmDialog
        open={pendingDelete !== null}
        title="Delete this mod?"
        description={
          pendingDelete
            ? `${pendingDelete.displayName} will be permanently deleted from ${modsDir ?? "the Mods folder"}. This cannot be undone.`
            : ""
        }
        confirmLabel="Delete folder"
        destructive
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          const mod = pendingDelete;
          setPendingDelete(null);
          if (mod) {
            if (mod.baseName === selectedBaseName) setSelectedBaseName(null);
            void removeMod(mod.folderName);
          }
        }}
      />
    </div>
  );
}

function FilterChip({
  active,
  label,
  count,
  tone = "default",
  onClick,
  children,
}: {
  active: boolean;
  label: string;
  count: number;
  tone?: "default" | "hot";
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={cn(
        "inline-flex h-9 items-center gap-1.5 rounded-lg border px-2.5 text-[11px] font-medium transition-colors duration-150",
        active
          ? tone === "hot"
            ? "border-hot-500/50 bg-hot-500/10 text-hot-400"
            : "border-acid-400/50 bg-acid-400/10 text-acid-300"
          : "border-ink-700 bg-ink-900 text-zinc-500 hover:border-ink-500 hover:text-zinc-300",
      )}
    >
      {children}
      <span className="hidden sm:inline">{label}</span>
      <span className="font-mono text-[10px] text-zinc-600">{count}</span>
    </button>
  );
}

function ToolTabButton({
  active,
  tone = "default",
  onClick,
  children,
}: {
  active: boolean;
  tone?: "default" | "hot";
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-expanded={active}
      className={cn(
        "inline-flex h-8 items-center gap-1.5 rounded-lg border px-2.5 text-[11px] font-medium transition-colors duration-150",
        active
          ? tone === "hot"
            ? "border-hot-500/50 bg-hot-500/10 text-hot-400"
            : "border-ink-600 bg-ink-800 text-zinc-100"
          : "border-transparent text-zinc-500 hover:bg-ink-900 hover:text-zinc-300",
      )}
    >
      {children}
    </button>
  );
}

function ViewButton({
  active,
  label,
  onClick,
  children,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      aria-pressed={active}
      className={cn(
        "flex h-8 w-8 items-center justify-center rounded-md transition-colors duration-150",
        active
          ? "bg-ink-700 text-zinc-100"
          : "text-zinc-500 hover:text-zinc-300",
      )}
    >
      {children}
    </button>
  );
}

function ScanProgressBar({ progress }: { progress: ModScanProgress | null }) {
  const percent =
    progress && progress.total > 0
      ? Math.round((progress.scanned / progress.total) * 100)
      : 0;

  return (
    <div className="mt-2.5">
      <div className="flex items-center justify-between gap-3">
        <span className="flex items-center gap-2 text-[11px] text-zinc-400">
          <Loader2 className="h-3 w-3 animate-spin text-acid-400" aria-hidden />
          Scanning Mods folder…
        </span>
        <span className="font-mono text-[10px] text-zinc-600">
          {progress && progress.total > 0
            ? `${progress.scanned} / ${progress.total}`
            : "…"}
        </span>
      </div>

      <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-ink-800">
        <div
          className={cn(
            "h-full rounded-full bg-acid-400 transition-[width] duration-150",
            percent === 0 && "w-1/3 animate-pulse",
          )}
          style={percent > 0 ? { width: `${percent}%` } : undefined}
        />
      </div>
    </div>
  );
}
