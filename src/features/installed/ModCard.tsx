import { useEffect, useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { FolderOpen, Keyboard, Loader2, Trash2 } from "lucide-react";

import { Badge } from "@/components/ui/Badge";
import { IconButton } from "@/components/ui/IconButton";
import { Switch } from "@/components/ui/Switch";
import { cn } from "@/lib/cn";
import type { InstalledMod } from "@/types/mods";

interface ModCardProps {
  mod: InstalledMod;
  busy: boolean;
  selected?: boolean;
  onSelect: () => void;
  onToggle: (enabled: boolean) => void;
  onOpen: () => void;
  onDelete: () => void;
}

function cardKeyboardHandler(onSelect: () => void) {
  return (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return;
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    onSelect();
  };
}

export function ModCard({
  mod,
  busy,
  selected = false,
  onSelect,
  onToggle,
  onOpen,
  onDelete,
}: ModCardProps) {
  const version = mod.metadata?.version?.trim();

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={`Open details for ${mod.displayName}`}
      aria-pressed={selected}
      onClick={onSelect}
      onKeyDown={cardKeyboardHandler(onSelect)}
      className={cn(
        "group relative flex cursor-pointer flex-col overflow-hidden rounded-2xl border bg-ink-900/80 transition-[transform,border-color,box-shadow] duration-200 ease-smooth hover:-translate-y-0.5 hover:border-ink-500/80 hover:shadow-[0_14px_34px_-24px_rgba(0,0,0,0.95)] motion-reduce:hover:translate-y-0",
        mod.enabled ? "border-ink-800" : "border-ink-800/60",
        selected && "border-acid-400/70 shadow-neon",
        mod.hasConflict && "border-hot-500/40 hover:border-hot-500/60",
        busy && "opacity-80",
      )}
    >
      <div className="relative aspect-[16/10] overflow-hidden bg-ink-850">
        <Thumbnail mod={mod} dimmed={!mod.enabled} />
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 bg-gradient-to-t from-ink-950/60 via-transparent to-ink-950/10"
        />

        <div className="absolute inset-x-0 top-0 flex flex-wrap items-start gap-1 p-2">
          {!mod.linked ? <Badge tone="cyan">Unlinked</Badge> : null}
          {mod.hasConflict ? <Badge tone="hot">Conflict</Badge> : null}
        </div>

        <div className="absolute inset-x-0 bottom-0 flex items-end justify-between gap-2 p-2">
          {mod.hotkeyCount > 0 ? (
            <span
              className="inline-flex items-center gap-1 rounded-full border border-white/10 bg-ink-950/75 px-2 py-0.5 font-mono text-[10px] leading-4 text-zinc-300 backdrop-blur"
              title={`${mod.hotkeyCount} hotkey${mod.hotkeyCount === 1 ? "" : "s"}`}
            >
              <Keyboard className="h-3 w-3 text-acid-300" aria-hidden />
              {mod.hotkeyCount}
            </span>
          ) : (
            <span />
          )}

          {version ? (
            <span className="rounded-full border border-white/10 bg-ink-950/70 px-2 py-0.5 font-mono text-[10px] leading-4 text-zinc-300 backdrop-blur">
              v{version}
            </span>
          ) : null}
        </div>
      </div>

      <div className="flex flex-1 flex-col px-3.5 pb-2.5 pt-2.5">
        <h3
          className="line-clamp-2 text-[13px] font-medium leading-snug text-zinc-100"
          title={mod.displayName}
        >
          {mod.displayName}
        </h3>
        <p
          className="mt-1 flex items-center gap-1.5 truncate text-[11px] text-zinc-500"
          title={mod.character || "Uncategorized"}
        >
          <span
            aria-hidden
            className={cn(
              "h-1.5 w-1.5 shrink-0 rounded-full",
              mod.enabled ? "bg-acid-400" : "bg-zinc-600",
            )}
          />
          <span className="truncate">
            {mod.character.trim() || "Uncategorized"}
          </span>
        </p>

        <div className="mt-auto flex items-center justify-between gap-2 pt-3">
          <div
            className="flex items-center gap-0.5 opacity-0 transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100"
            onClick={(event) => event.stopPropagation()}
          >
            {busy ? (
              <Loader2
                className="mr-1 h-3.5 w-3.5 animate-spin text-acid-400"
                aria-hidden
              />
            ) : (
              <>
                <IconButton label="Open folder" onClick={onOpen} disabled={busy}>
                  <FolderOpen className="h-3.5 w-3.5" aria-hidden />
                </IconButton>
                <IconButton
                  label="Delete mod"
                  onClick={onDelete}
                  disabled={busy}
                  tone="danger"
                >
                  <Trash2 className="h-3.5 w-3.5" aria-hidden />
                </IconButton>
              </>
            )}
          </div>

          <div
            className="flex items-center gap-2"
            onClick={(event) => event.stopPropagation()}
          >
            <span
              className={cn(
                "text-[10px] font-medium",
                mod.enabled ? "text-acid-300/80" : "text-zinc-600",
              )}
            >
              {mod.enabled ? "Enabled" : "Disabled"}
            </span>
            <Switch
              checked={mod.enabled}
              onCheckedChange={onToggle}
              disabled={busy}
              label={`${mod.enabled ? "Disable" : "Enable"} ${mod.displayName}`}
            />
          </div>
        </div>
      </div>
    </article>
  );
}

/** Dense horizontal variant used by the list view. */
export function ModRow({
  mod,
  busy,
  selected = false,
  onSelect,
  onToggle,
  onOpen,
  onDelete,
}: ModCardProps) {
  const version = mod.metadata?.version?.trim();

  return (
    <article
      role="button"
      tabIndex={0}
      aria-label={`Open details for ${mod.displayName}`}
      aria-pressed={selected}
      onClick={onSelect}
      onKeyDown={cardKeyboardHandler(onSelect)}
      className={cn(
        "group flex cursor-pointer items-center gap-3 rounded-xl border bg-ink-900/60 px-2.5 py-2 transition-colors duration-150 ease-smooth hover:border-ink-500/80 hover:bg-ink-900",
        mod.enabled ? "border-ink-800" : "border-ink-800/60",
        selected && "border-acid-400/70 shadow-neon",
        mod.hasConflict && "border-hot-500/40",
        busy && "opacity-80",
      )}
    >
      <div className="relative h-11 w-[4.5rem] shrink-0 overflow-hidden rounded-lg bg-ink-850">
        <Thumbnail mod={mod} dimmed={!mod.enabled} compact />
      </div>

      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <h3 className="truncate text-[12.5px] font-medium text-zinc-100">
            {mod.displayName}
          </h3>
          {!mod.linked ? <Badge tone="cyan">Unlinked</Badge> : null}
          {mod.hasConflict ? <Badge tone="hot">Conflict</Badge> : null}
        </div>
        <p className="mt-0.5 flex min-w-0 items-center gap-1.5 truncate text-[11px] text-zinc-500">
          <span
            aria-hidden
            className={cn(
              "h-1.5 w-1.5 shrink-0 rounded-full",
              mod.enabled ? "bg-acid-400" : "bg-zinc-600",
            )}
          />
          <span className="truncate">{mod.character.trim() || "Uncategorized"}</span>
          <span className="text-zinc-700">·</span>
          <span className="truncate font-mono text-[10px]">{mod.folderName}</span>
          {version ? (
            <>
              <span className="text-zinc-700">·</span>
              <span className="font-mono text-[10px]">v{version}</span>
            </>
          ) : null}
          {mod.hotkeyCount > 0 ? (
            <>
              <span className="text-zinc-700">·</span>
              <span
                className="inline-flex shrink-0 items-center gap-1 font-mono text-[10px] text-zinc-400"
                title={`${mod.hotkeyCount} hotkey${mod.hotkeyCount === 1 ? "" : "s"}`}
              >
                <Keyboard className="h-3 w-3 text-acid-300" aria-hidden />
                {mod.hotkeyCount}
              </span>
            </>
          ) : null}
        </p>
      </div>

      <div
        className="flex shrink-0 items-center gap-1 opacity-0 transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100"
        onClick={(event) => event.stopPropagation()}
      >
        {busy ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin text-acid-400" aria-hidden />
        ) : (
          <>
            <IconButton label="Open folder" onClick={onOpen} disabled={busy}>
              <FolderOpen className="h-3.5 w-3.5" aria-hidden />
            </IconButton>
            <IconButton
              label="Delete mod"
              onClick={onDelete}
              disabled={busy}
              tone="danger"
            >
              <Trash2 className="h-3.5 w-3.5" aria-hidden />
            </IconButton>
          </>
        )}
      </div>

      <div
        className="flex shrink-0 items-center gap-2"
        onClick={(event) => event.stopPropagation()}
      >
        <span
          className={cn(
            "hidden text-[10px] font-medium sm:inline",
            mod.enabled ? "text-acid-300/80" : "text-zinc-600",
          )}
        >
          {mod.enabled ? "Enabled" : "Disabled"}
        </span>
        <Switch
          checked={mod.enabled}
          onCheckedChange={onToggle}
          disabled={busy}
          label={`${mod.enabled ? "Disable" : "Enable"} ${mod.displayName}`}
        />
      </div>
    </article>
  );
}

function Thumbnail({
  mod,
  dimmed,
  compact = false,
}: {
  mod: InstalledMod;
  dimmed: boolean;
  compact?: boolean;
}) {
  const [localFailed, setLocalFailed] = useState(false);
  const [remoteFailed, setRemoteFailed] = useState(false);

  useEffect(() => {
    setLocalFailed(false);
    setRemoteFailed(false);
  }, [mod.folderName, mod.thumbnailPath]);

  const localSrc = useMemo(() => {
    if (!mod.thumbnailPath || localFailed) return null;
    return convertFileSrc(mod.thumbnailPath);
  }, [mod.thumbnailPath, localFailed]);

  const remoteSrc =
    !remoteFailed && mod.metadata?.thumbnailUrl ? mod.metadata.thumbnailUrl : null;

  const src = localSrc ?? remoteSrc;

  if (!src) {
    return (
      <div className="relative flex h-full w-full flex-col items-center justify-center gap-1 overflow-hidden bg-gradient-to-br from-ink-800 via-ink-850 to-ink-900">
        <span
          aria-hidden
          className="absolute inset-0 opacity-[0.04]"
          style={{
            backgroundImage:
              "repeating-linear-gradient(135deg, #ffffff 0 1px, transparent 1px 12px)",
          }}
        />
        <span
          className={cn(
            "relative flex items-center justify-center rounded-full border font-display font-semibold",
            compact ? "h-6 w-6 text-[11px]" : "h-10 w-10 text-base",
            dimmed
              ? "border-ink-600/60 bg-ink-800/60 text-zinc-600"
              : "border-acid-400/25 bg-acid-400/[0.07] text-acid-300/80",
          )}
        >
          {initials(mod)}
        </span>
        {compact ? null : (
          <span className="relative text-[8px] uppercase tracking-[0.18em] text-zinc-600">
            No preview
          </span>
        )}
      </div>
    );
  }

  return (
    <img
      src={src}
      alt=""
      loading="lazy"
      decoding="async"
      className={cn(
        "h-full w-full object-cover transition-transform duration-300 ease-smooth group-hover:scale-[1.03] motion-reduce:transform-none",
        dimmed && "grayscale",
      )}
      onError={() => {
        if (localSrc) setLocalFailed(true);
        else setRemoteFailed(true);
      }}
    />
  );
}

function initials(mod: InstalledMod): string {
  const source = mod.displayName.trim() || mod.baseName;
  return source.slice(0, 1).toUpperCase();
}
