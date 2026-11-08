import { useEffect, useMemo, useState } from "react";
import {
  ArrowLeft,
  CalendarDays,
  Download,
  Heart,
  Loader2,
  Package,
  X,
  type LucideIcon,
} from "lucide-react";
import { useNavigate, useParams } from "react-router-dom";

import { Button } from "@/components/ui/Button";
import { Notice } from "@/components/ui/Notice";
import { Panel } from "@/components/ui/Panel";
import { cn } from "@/lib/cn";
import { formatGameBananaDescription } from "@/lib/gamebananaText";
import { formatBytes, formatCount, formatDate } from "@/lib/format";
import { useBrowseStore } from "@/store/browse";
import { useDownloadStore } from "@/store/download";

export function ModDetailPage() {
  const { modId } = useParams<{ modId: string }>();
  const id = Number(modId);

  const detail = useBrowseStore((state) => state.details[id]);
  const loadingId = useBrowseStore((state) => state.detailLoadingId);
  const detailError = useBrowseStore((state) => state.detailError);
  const loadDetail = useBrowseStore((state) => state.loadDetail);

  const isBusy = useDownloadStore((state) => state.isBusy);
  const activeModId = useDownloadStore((state) => state.activeModId);
  const progress = useDownloadStore((state) => state.progress);
  const install = useDownloadStore((state) => state.install);
  const cancel = useDownloadStore((state) => state.cancel);
  const cancelling = useDownloadStore((state) => state.cancelling);

  const navigate = useNavigate();
  const [selectedFileId, setSelectedFileId] = useState<number | null>(null);
  const [activeImage, setActiveImage] = useState(0);
  const [dismissedError, setDismissedError] = useState<string | null>(null);

  const description = useMemo(
    () => formatGameBananaDescription(detail?.description),
    [detail?.description],
  );

  useEffect(() => {
    setDismissedError(null);
  }, [id]);

  useEffect(() => {
    if (Number.isFinite(id) && id > 0) void loadDetail(id);
  }, [id, loadDetail]);

  // Default to the newest file once the detail arrives.
  useEffect(() => {
    if (!detail) return;
    setSelectedFileId((current) =>
      current !== null && detail.files.some((file) => file.id === current)
        ? current
        : (detail.files[0]?.id ?? null),
    );
    setActiveImage(0);
  }, [detail]);

  const busyHere = isBusy && activeModId === id;
  const selectedFile = detail?.files.find((file) => file.id === selectedFileId) ?? null;

  const goBack = () => navigate("/browse");

  if (!Number.isFinite(id) || id <= 0) {
    return (
      <div className="mx-auto max-w-3xl px-8 py-8">
        <Panel eyebrow="Invalid link" title="No mod selected">
          <Button variant="primary" onClick={goBack}>
            Back to browse
          </Button>
        </Panel>
      </div>
    );
  }

  if (!detail) {
    return (
      <div className="mx-auto w-full max-w-[1500px] px-8 py-8">
        <Button variant="ghost" size="sm" onClick={goBack}>
          <ArrowLeft className="h-3.5 w-3.5" aria-hidden />
          Back to browse
        </Button>

        {detailError && dismissedError !== detailError ? (
          <Notice
            className="mt-6"
            title="Could not load this mod"
            message={detailError}
            onDismiss={() => setDismissedError(detailError)}
            action={
              <Button
                size="sm"
                variant="outline"
                onClick={() => void loadDetail(id)}
              >
                Try again
              </Button>
            }
          />
        ) : (
          <div className="mt-6 flex items-center gap-3 border border-ink-700 bg-ink-900/60 px-4 py-6 text-sm text-zinc-400">
            <Loader2
              className={cn(
                "h-4 w-4 text-acid-400",
                loadingId === id && "animate-spin",
              )}
              aria-hidden
            />
            Loading mod details…
          </div>
        )}
      </div>
    );
  }

  const images = detail.gallery;
  const hero = images[activeImage] ?? detail.thumbnailUrl;

  return (
    <div className="mx-auto w-full max-w-[1500px] px-8 py-8">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Button variant="ghost" size="sm" onClick={goBack}>
          <ArrowLeft className="h-3.5 w-3.5" aria-hidden />
          Back to browse
        </Button>
        <div className="flex flex-wrap items-center gap-2 font-mono text-[10px] text-zinc-600">
          <span>GameBanana ID {detail.id}</span>
          {detail.cached ? <span>· cached</span> : null}
          {detail.obsolete ? (
            <span className="text-hot-400">· marked obsolete</span>
          ) : null}
        </div>
      </div>

      <div className="mt-5 grid gap-6 lg:grid-cols-[minmax(0,1.35fr)_minmax(320px,1fr)]">
        <div>
          <div className="relative aspect-[16/9] overflow-hidden border border-ink-700 bg-ink-850">
            {hero ? (
              <img
                src={hero}
                alt={`${detail.name} preview`}
                className="h-full w-full object-contain"
              />
            ) : (
              <div className="flex h-full items-center justify-center">
                <Package className="h-8 w-8 text-zinc-700" aria-hidden />
              </div>
            )}
          </div>

          {images.length > 1 ? (
            <div className="mt-2 flex gap-2 overflow-x-auto pb-1">
              {images.map((image, index) => (
                <button
                  key={image}
                  type="button"
                  onClick={() => setActiveImage(index)}
                  aria-label={`Show image ${index + 1}`}
                  aria-pressed={index === activeImage}
                  className={cn(
                    "h-14 w-24 shrink-0 overflow-hidden border",
                    index === activeImage
                      ? "border-acid-400"
                      : "border-ink-700 hover:border-ink-500",
                  )}
                >
                  <img
                    src={image}
                    alt=""
                    loading="lazy"
                    className="h-full w-full object-cover"
                  />
                </button>
              ))}
            </div>
          ) : null}

          <Panel
            className="mt-5"
            eyebrow="Description"
            title="About this mod"
          >
            {description ? (
              <div className="max-h-[520px] overflow-y-auto pr-2">
                <p className="whitespace-pre-wrap break-words text-[12.5px] leading-6 text-zinc-400">
                  {description}
                </p>
              </div>
            ) : (
              <p className="text-xs text-zinc-600">
                The author did not write a description for this mod.
              </p>
            )}
          </Panel>
        </div>

        <div className="space-y-4">
          <div>
            <p className="eyebrow">
              {[detail.category, detail.subCategory].filter(Boolean).join(" // ") ||
                "Uncategorized"}
            </p>
            <h1 className="mt-1.5 font-display text-[26px] font-semibold leading-tight tracking-tight text-zinc-50">
              {detail.name}
            </h1>
            <p className="mt-1.5 text-xs text-zinc-500">
              by <span className="text-zinc-300">{detail.author ?? "unknown"}</span>
              {detail.version ? (
                <span className="ml-2 font-mono text-[10px] text-zinc-500">
                  {detail.version}
                </span>
              ) : null}
            </p>
          </div>

          <div className="grid grid-cols-2 gap-2">
            <Stat
              icon={Heart}
              label="Likes"
              value={formatCount(detail.likes)}
              tone="hot"
            />
            <Stat
              icon={Download}
              label="Downloads"
              value={formatCount(detail.downloads)}
            />
            <Stat label="Views" value={formatCount(detail.views)} />
            <Stat
              icon={CalendarDays}
              label="Updated"
              value={formatDate(detail.dateUpdated)}
            />
          </div>

          <Panel eyebrow="Install" title="Files">
            {detail.files.length === 0 ? (
              <p className="text-xs leading-relaxed text-zinc-500">
                This mod has no downloadable files on GameBanana, so ZZMM cannot
                install it automatically.
              </p>
            ) : (
              <div className="space-y-2">
                {detail.files.map((file) => {
                  const selected = file.id === selectedFileId;

                  return (
                    <button
                      key={file.id}
                      type="button"
                      onClick={() => setSelectedFileId(file.id)}
                      aria-pressed={selected}
                      className={cn(
                        "flex w-full flex-col gap-0.5 border px-3 py-2 text-left transition-colors",
                        selected
                          ? "border-acid-400/70 bg-acid-400/[0.06]"
                          : "border-ink-700 bg-ink-850/60 hover:border-ink-500",
                      )}
                    >
                      <span className="flex items-center justify-between gap-2">
                        <span
                          className="truncate font-mono text-[11px] text-zinc-200"
                          title={file.name}
                        >
                          {file.name}
                        </span>
                        <span className="shrink-0 font-mono text-[9px] text-zinc-500">
                          {formatBytes(file.size)}
                        </span>
                      </span>
                      <span className="flex flex-wrap gap-x-2 font-mono text-[9px] text-zinc-600">
                        {file.version ? <span>{file.version}</span> : null}
                        <span>{formatDate(file.date)}</span>
                        {file.description ? <span>{file.description}</span> : null}
                      </span>
                    </button>
                  );
                })}

                <Button
                  variant="primary"
                  className="mt-1 w-full"
                  loading={busyHere}
                  disabled={!selectedFile || isBusy}
                  onClick={() => {
                    if (!selectedFile) return;
                    void install({
                      modId: detail.id,
                      name: detail.name,
                      fileId: selectedFile.id,
                    });
                  }}
                >
                  <Download className="h-3.5 w-3.5" aria-hidden />
                  Install to Mods folder
                </Button>

                {busyHere && progress ? (
                  <div className="border border-ink-700 bg-ink-850/60 px-3 py-2">
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-display text-[9px] uppercase tracking-[0.18em] text-acid-300">
                        {progress.stage === "installing" ? "Installing" : "Downloading"}
                      </span>
                      <span className="font-mono text-[9px] text-zinc-500">
                        {progress.percent}%
                      </span>
                    </div>
                    <div className="mt-1.5 h-1 overflow-hidden bg-ink-800">
                      <div
                        className="h-full bg-acid-400 transition-[width] duration-150"
                        style={{
                          width: `${Math.min(100, Math.max(2, progress.percent))}%`,
                        }}
                      />
                    </div>
                    <div className="mt-1.5 flex items-center justify-between gap-2">
                      <p className="min-w-0 flex-1 truncate text-[10px] text-zinc-500">
                        {progress.message}
                      </p>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-6 shrink-0 px-2 text-[10px]"
                        onClick={() => void cancel()}
                        disabled={cancelling}
                      >
                        <X className="h-3 w-3" aria-hidden />
                        {cancelling ? "Cancelling…" : "Cancel"}
                      </Button>
                    </div>
                  </div>
                ) : null}

                <p className="text-[10px] leading-relaxed text-zinc-600">
                  The archive is extracted into your Mods folder and tagged with
                  this mod and file ID so update checks can find it later. If a
                  previous version is updated from the Installed page, the old
                  folder is disabled rather than deleted.
                </p>
              </div>
            )}
          </Panel>
        </div>
      </div>
    </div>
  );
}

function Stat({
  icon: Icon,
  label,
  value,
  tone = "muted",
}: {
  icon?: LucideIcon;
  label: string;
  value: string;
  tone?: "hot" | "muted";
}) {
  return (
    <div className="border border-ink-700/80 bg-ink-900/60 px-3 py-2">
      <p className="flex items-center gap-1 font-display text-[9px] uppercase tracking-[0.18em] text-zinc-500">
        {Icon ? (
          <Icon
            className={cn("h-2.5 w-2.5", tone === "hot" ? "text-hot-400" : "text-zinc-600")}
            aria-hidden
          />
        ) : null}
        {label}
      </p>
      <p className="mt-1 font-display text-base font-semibold leading-none text-zinc-200">
        {value}
      </p>
    </div>
  );
}
