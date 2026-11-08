import { useEffect, useMemo, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { LucideIcon } from "lucide-react";
import {
  Compass,
  Download,
  Layers,
  Loader2,
  Search,
  Settings2,
  Users,
  X,
} from "lucide-react";
import {
  Link,
  NavLink,
  Outlet,
  useLocation,
  useNavigate,
  useSearchParams,
} from "react-router-dom";

import appIcon from "@/assets/app-icon.png";
import { Button } from "@/components/ui/Button";
import { ToastViewport } from "@/components/ui/Toast";
import { characterIcon } from "@/lib/characterIcons";
import { cn } from "@/lib/cn";
import { useInstallerStore } from "@/store/installer";
import { useModsStore } from "@/store/mods";
import { useSettingsStore } from "@/store/settings";
import { toast } from "@/store/toasts";
import { CharacterPrompt } from "./CharacterPrompt";

interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
}

interface CharacterEntry {
  character: string;
  count: number;
}

const NAV_ITEMS: NavItem[] = [
  { to: "/installed", label: "Installed", icon: Layers },
  { to: "/browse", label: "Browse", icon: Compass },
  { to: "/settings", label: "Settings", icon: Settings2 },
];

type StatusFilter = "all" | "enabled" | "disabled";

const STATUS_OPTIONS: { value: StatusFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "enabled", label: "Enabled" },
  { value: "disabled", label: "Disabled" },
];

const ARCHIVE_EXTENSIONS = [".zip", ".7z", ".rar"];

function isSupportedArchive(path: string): boolean {
  const lower = path.toLowerCase();
  return ARCHIVE_EXTENSIONS.some((extension) => lower.endsWith(extension));
}

export function AppShell() {
  const view = useSettingsStore((state) => state.view);
  const install = useInstallerStore((state) => state.install);
  const cancelInstall = useInstallerStore((state) => state.cancel);
  const isInstalling = useInstallerStore((state) => state.isInstalling);
  const cancelling = useInstallerStore((state) => state.cancelling);
  const installProgress = useInstallerStore((state) => state.progress);
  const installedCount = useModsStore((state) => state.result?.mods.length ?? null);
  const mods = useModsStore((state) => state.result?.mods);
  const assignCharacter = useModsStore((state) => state.assignCharacter);

  const navigate = useNavigate();
  const location = useLocation();
  const [searchParams, setSearchParams] = useSearchParams();
  const [dragActive, setDragActive] = useState(false);
  const [assignFolders, setAssignFolders] = useState<string[] | null>(null);
  const [assigning, setAssigning] = useState(false);

  const configured = Boolean(view?.settings.modsDir);
  const valid = view?.modsDirState.valid ?? false;
  const statusLabel = !configured
    ? "No mods folder"
    : valid
      ? "Mods linked"
      : "Folder missing";
  const statusColor = !configured
    ? "bg-zinc-600"
    : valid
      ? "bg-acid-400"
      : "bg-hot-500";

  const characters = useMemo<CharacterEntry[]>(() => {
    const counts = new Map<string, number>();

    for (const mod of mods ?? []) {
      const character = mod.character.trim() || "Uncategorized";
      counts.set(character, (counts.get(character) ?? 0) + 1);
    }

    return [...counts.entries()]
      .map(([character, count]) => ({ character, count }))
      .sort((a, b) => {
        if (a.character === "Uncategorized") return 1;
        if (b.character === "Uncategorized") return -1;
        return a.character.localeCompare(b.character);
      });
  }, [mods]);

  const [characterFilter, setCharacterFilter] = useState("");

  const filteredCharacters = useMemo(() => {
    const term = characterFilter.trim().toLowerCase();
    if (!term) return characters;
    return characters.filter((entry) =>
      entry.character.toLowerCase().includes(term),
    );
  }, [characters, characterFilter]);

  const activeCharacter =
    location.pathname === "/installed"
      ? searchParams.get("character")
      : null;
  const isInstalled = location.pathname === "/installed";
  const query = isInstalled ? (searchParams.get("q") ?? "") : "";
  const statusParam = isInstalled ? searchParams.get("state") : null;
  const status: StatusFilter =
    statusParam === "enabled" || statusParam === "disabled" ? statusParam : "all";
  const [searchInput, setSearchInput] = useState(query);

  useEffect(() => {
    setSearchInput(query);
  }, [query]);

  const updateInstalledParams = (patch: Record<string, string | null>) => {
    const next = new URLSearchParams(searchParams);
    for (const [key, value] of Object.entries(patch)) {
      if (!value) next.delete(key);
      else next.set(key, value);
    }
    setSearchParams(next, { replace: true });
  };

  const characterHref = (character: string) => {
    const next = new URLSearchParams(isInstalled ? searchParams : undefined);
    if (character === "all") next.delete("character");
    else next.set("character", character);
    const queryString = next.toString();
    return `/installed${queryString ? `?${queryString}` : ""}`;
  };

  // Drag-and-drop is global: archives can be dropped on any page and are
  // installed into the configured Mods folder.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;

        if (payload.type === "enter" || payload.type === "over") {
          setDragActive(true);
          return;
        }

        if (payload.type === "leave") {
          setDragActive(false);
          return;
        }

        if (payload.type === "drop") {
          setDragActive(false);

          if (useInstallerStore.getState().isInstalling) return;

          const archives = payload.paths.filter(isSupportedArchive);

          if (archives.length === 0) {
            toast.error(
              "Unsupported drop",
              "Drop .zip, .7z, or .rar archives to install a mod.",
            );
            return;
          }

          void install(archives).then((folders) => {
            if (folders.length > 0) {
              navigate("/installed");
              // Naming the character now makes the sidebar filter and the
              // one-active-per-character rule useful immediately.
              setAssignFolders(folders);
            }
          });
        }
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // Browser-only preview: the Tauri drag/drop bridge is unavailable.
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [install, navigate]);

  return (
    <div className="flex h-screen overflow-hidden">
      <aside className="flex w-[260px] shrink-0 flex-col border-r border-ink-700/70 bg-ink-950">
        <div className="flex items-center gap-3 px-4 pb-4 pt-5">
          <img
            src={appIcon}
            alt=""
            draggable={false}
            className="h-10 w-10 shrink-0 select-none object-contain drop-shadow-[0_0_14px_rgba(210,255,60,0.18)]"
          />
          <div className="min-w-0">
            <p className="font-display text-[16px] font-semibold leading-none tracking-tight text-zinc-50">
              ZZMM
            </p>
            <p className="mt-1 truncate text-[11px] leading-tight text-zinc-500">
              Zenless Zone Mod Manager
            </p>
          </div>
        </div>

        <nav
          className="flex flex-col gap-0.5 px-2"
          aria-label="Main navigation"
        >
          {NAV_ITEMS.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              className={({ isActive }) =>
                cn(
                  "group relative flex h-9 items-center gap-2.5 rounded-lg px-2.5 text-[13px] font-medium transition-colors duration-150 ease-smooth",
                  isActive
                    ? "bg-ink-800 text-zinc-50"
                    : "text-zinc-400 hover:bg-ink-800/60 hover:text-zinc-200",
                )
              }
            >
              {({ isActive }) => (
                <>
                  <span
                    aria-hidden
                    className={cn(
                      "absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full bg-acid-400 transition-opacity duration-150",
                      isActive ? "opacity-100" : "opacity-0",
                    )}
                  />
                  <item.icon
                    aria-hidden
                    className={cn(
                      "h-4 w-4 shrink-0 transition-colors",
                      isActive
                        ? "text-acid-400"
                        : "text-zinc-500 group-hover:text-zinc-300",
                    )}
                  />
                  <span className="flex-1 truncate">{item.label}</span>
                  {item.to === "/installed" && installedCount !== null ? (
                    <span className="rounded-full bg-ink-700 px-1.5 py-0.5 font-mono text-[10px] leading-none text-zinc-400">
                      {installedCount}
                    </span>
                  ) : null}
                </>
              )}
            </NavLink>
          ))}
        </nav>

        {isInstalled || characters.length > 0 ? (
          <div className="mt-2 flex min-h-0 flex-1 flex-col border-t border-ink-700/60 px-2 pt-3">
            {isInstalled ? (
              <div className="space-y-2 px-0.5">
                <label className="relative flex h-9 items-center gap-2 rounded-lg border border-ink-700 bg-ink-900 px-2.5 transition-colors focus-within:border-ink-500">
                  <Search
                    className="h-3.5 w-3.5 shrink-0 text-zinc-500"
                    aria-hidden
                  />
                  <input
                    type="search"
                    value={searchInput}
                    onChange={(event) => {
                      const value = event.target.value;
                      setSearchInput(value);
                      updateInstalledParams({ q: value || null });
                    }}
                    placeholder="Search library…"
                    aria-label="Search installed mods"
                    className="h-full w-full bg-transparent text-[12px] text-zinc-200 placeholder:text-zinc-600 focus:outline-none"
                  />
                  {searchInput ? (
                    <button
                      type="button"
                      onClick={() => {
                        setSearchInput("");
                        updateInstalledParams({ q: null });
                      }}
                      aria-label="Clear search"
                      className="rounded p-0.5 text-zinc-600 transition-colors hover:text-zinc-300"
                    >
                      <X className="h-3.5 w-3.5" aria-hidden />
                    </button>
                  ) : null}
                </label>

                <div className="flex items-center rounded-lg border border-ink-700 bg-ink-900 p-0.5">
                  {STATUS_OPTIONS.map((option) => (
                    <button
                      key={option.value}
                      type="button"
                      onClick={() =>
                        updateInstalledParams({
                          state: option.value === "all" ? null : option.value,
                        })
                      }
                      aria-pressed={status === option.value}
                      className={cn(
                        "h-8 flex-1 rounded-md text-[11px] font-medium transition-colors duration-150",
                        status === option.value
                          ? "bg-ink-700 text-zinc-100"
                          : "text-zinc-500 hover:text-zinc-300",
                      )}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              </div>
            ) : null}

            {characters.length > 0 ? (
              <>
                <div className="mt-3 flex items-center justify-between px-2.5">
                  <p className="eyebrow">Characters</p>
                  <span className="font-mono text-[10px] text-zinc-600">
                    {characters.length}
                  </span>
                </div>
                {characters.length > 6 ? (
                  <label className="relative mx-0.5 mt-2 flex h-7 items-center gap-1.5 rounded-md border border-ink-800 bg-ink-900/70 px-2 transition-colors focus-within:border-ink-600">
                    <Search
                      className="h-3 w-3 shrink-0 text-zinc-600"
                      aria-hidden
                    />
                    <input
                      type="search"
                      value={characterFilter}
                      onChange={(event) =>
                        setCharacterFilter(event.target.value)
                      }
                      placeholder="Filter characters…"
                      aria-label="Filter characters"
                      className="h-full w-full bg-transparent text-[11px] text-zinc-300 placeholder:text-zinc-600 focus:outline-none"
                    />
                    {characterFilter ? (
                      <button
                        type="button"
                        onClick={() => setCharacterFilter("")}
                        aria-label="Clear character filter"
                        className="rounded p-0.5 text-zinc-600 transition-colors hover:text-zinc-300"
                      >
                        <X className="h-3 w-3" aria-hidden />
                      </button>
                    ) : null}
                  </label>
                ) : null}

                <div className="mt-1.5 min-h-0 flex-1 space-y-0.5 overflow-y-auto pr-0.5">
                  <Link
                    to={characterHref("all")}
                    className={cn(
                      "group flex h-8 items-center gap-2 rounded-lg px-2 text-[12px] transition-colors duration-150 ease-smooth",
                      activeCharacter === null
                        ? "bg-ink-800 text-zinc-50"
                        : "text-zinc-400 hover:bg-ink-800/60 hover:text-zinc-200",
                    )}
                  >
                    <span
                      aria-hidden
                      className={cn(
                        "flex h-6 w-6 shrink-0 items-center justify-center rounded-full border",
                        activeCharacter === null
                          ? "border-acid-400/50 bg-acid-400/10 text-acid-300"
                          : "border-ink-700 bg-ink-900 text-zinc-500",
                      )}
                    >
                      <Users className="h-3 w-3" />
                    </span>
                    <span className="min-w-0 flex-1 truncate">All characters</span>
                    <span className="font-mono text-[10px] text-zinc-600">
                      {installedCount ?? 0}
                    </span>
                  </Link>

                  {filteredCharacters.map(({ character, count }) => (
                    <Link
                      key={character}
                      to={characterHref(character)}
                      title={character}
                      className={cn(
                        "group flex h-8 items-center gap-2 rounded-lg px-2 text-[12px] transition-colors duration-150 ease-smooth",
                        activeCharacter === character
                          ? "bg-ink-800 text-zinc-50"
                          : "text-zinc-400 hover:bg-ink-800/60 hover:text-zinc-200",
                      )}
                    >
                      <CharacterAvatar
                        character={character}
                        active={activeCharacter === character}
                      />
                      <span className="min-w-0 flex-1 truncate">{character}</span>
                      <span className="font-mono text-[10px] text-zinc-600">
                        {count}
                      </span>
                    </Link>
                  ))}

                  {filteredCharacters.length === 0 ? (
                    <p className="px-2 py-3 text-[11px] leading-relaxed text-zinc-600">
                      No character matches “{characterFilter.trim()}”.
                    </p>
                  ) : null}
                </div>
              </>
            ) : null}
          </div>
        ) : null}

        <div className="mt-2 space-y-0.5 border-t border-ink-700/60 px-2 py-3">
          <Link
            to="/settings"
            className="flex h-8 items-center gap-2 rounded-lg px-2.5 text-[11px] text-zinc-500 transition-colors duration-150 hover:bg-ink-800/60 hover:text-zinc-300"
          >
            <span aria-hidden className={cn("h-1.5 w-1.5 rounded-full", statusColor)} />
            {statusLabel}
          </Link>
          <p className="px-2.5 pt-1 font-mono text-[10px] text-zinc-600">
            {view ? `v${view.paths.appVersion}` : "—"}
          </p>
        </div>
      </aside>

      <main className="relative flex-1 overflow-y-auto">
        <Outlet />
      </main>

      <ToastViewport />

      {assignFolders ? (
        <CharacterPrompt
          folders={assignFolders}
          suggestions={characters
            .map((entry) => entry.character)
            .filter((character) => character !== "Uncategorized")}
          busy={assigning}
          onSkip={() => setAssignFolders(null)}
          onAssign={async (character) => {
            setAssigning(true);
            const saved = await assignCharacter(assignFolders, character);
            setAssigning(false);
            if (saved) setAssignFolders(null);
          }}
        />
      ) : null}

      {dragActive ? (
        <div className="pointer-events-none fixed inset-0 z-[55] flex items-center justify-center bg-ink-950/80 p-8 backdrop-blur-sm">
          <div className="animate-fade-in rounded-2xl border-2 border-dashed border-acid-400/60 bg-ink-900/90 px-12 py-10 text-center shadow-pop">
            <Download className="mx-auto h-8 w-8 text-acid-300" aria-hidden />
            <p className="mt-4 font-display text-base font-semibold text-zinc-50">
              Drop to install
            </p>
            <p className="mt-1.5 text-[12px] text-zinc-500">
              .zip · .7z · .rar — multiple mods supported
            </p>
          </div>
        </div>
      ) : null}

      {isInstalling ? (
        <div className="fixed bottom-5 left-[276px] z-50 w-[340px] max-w-[calc(100vw-19rem)] animate-toast-in rounded-xl border border-ink-700 bg-ink-900/95 p-3.5 shadow-pop backdrop-blur">
          <div className="flex items-center justify-between gap-3">
            <span className="flex items-center gap-2 text-[12px] font-medium text-zinc-100">
              <Loader2 className="h-3.5 w-3.5 animate-spin text-acid-400" aria-hidden />
              Installing archive
            </span>
            <span className="font-mono text-[10px] text-zinc-500">
              {installProgress?.percent ?? 0}%
            </span>
          </div>
          <p
            className="mt-1.5 truncate font-mono text-[10px] text-zinc-500"
            title={installProgress?.archive}
          >
            {installProgress?.archive || "Preparing archive…"}
          </p>
          <div className="mt-2 h-1 overflow-hidden rounded-full bg-ink-800">
            <div
              className="h-full rounded-full bg-acid-400 transition-[width] duration-150"
              style={{
                width: `${Math.min(100, Math.max(2, installProgress?.percent ?? 2))}%`,
              }}
            />
          </div>
          <div className="mt-1.5 flex items-center justify-between gap-2">
            <p className="min-w-0 flex-1 truncate text-[10px] text-zinc-500">
              {installProgress?.message ?? "Working…"}
            </p>
            <Button
              size="sm"
              variant="ghost"
              className="h-6 shrink-0 px-2 text-[10px]"
              onClick={() => void cancelInstall()}
              disabled={cancelling}
            >
              <X className="h-3 w-3" aria-hidden />
              {cancelling ? "Cancelling…" : "Cancel"}
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function CharacterAvatar({
  character,
  active,
}: {
  character: string;
  active: boolean;
}) {
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
  }, [character]);

  const src = useMemo(
    () => (failed ? null : characterIcon(character)),
    [character, failed],
  );

  return (
    <span
      aria-hidden
      className={cn(
        "flex h-6 w-6 shrink-0 items-center justify-center overflow-hidden rounded-full border font-mono text-[9px]",
        active
          ? "border-acid-400/50 bg-acid-400/10 text-acid-300"
          : "border-ink-700 bg-ink-900 text-zinc-500",
      )}
    >
      {src ? (
        <img
          src={src}
          alt=""
          draggable={false}
          className="h-full w-full object-cover"
          onError={() => setFailed(true)}
        />
      ) : (
        character.slice(0, 1).toUpperCase()
      )}
    </span>
  );
}
