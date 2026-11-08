import { Panel } from "@/components/ui/Panel";
import type { HashConflict } from "@/types/mods";

interface HashConflictsProps {
  conflicts: HashConflict[];
}

export function HashConflicts({ conflicts }: HashConflictsProps) {
  const count = conflicts.length;

  return (
    <Panel
      eyebrow="Load-order risk"
      title={`${count} hash conflict${count === 1 ? "" : "s"}`}
      description="Enabled mods below override the same texture hash. 3DMigoto only uses one of them, so textures may be missing or mixed until you disable one."
    >
      <div className="max-h-[420px] space-y-1.5 overflow-y-auto pr-1">
        {conflicts.map((conflict) => {
          const distinctMods = new Set(
            conflict.mods.map((part) => part.folderName),
          ).size;

          return (
            <details
              key={conflict.hash}
              className="group rounded-lg border border-hot-500/15 bg-hot-500/[0.025] open:border-hot-500/30"
            >
              <summary className="flex cursor-pointer list-none items-center justify-between gap-3 rounded-lg px-3 py-2 transition-colors hover:bg-hot-500/[0.04] [&::-webkit-details-marker]:hidden">
                <span className="font-mono text-[11px] text-hot-400">
                  {conflict.hash}
                </span>
                <span className="font-display text-[9px] uppercase tracking-[0.18em] text-zinc-500">
                  {distinctMods} mod{distinctMods === 1 ? "" : "s"}
                </span>
              </summary>

              <ul className="space-y-1 border-t border-hot-500/15 px-3 py-2">
                {conflict.mods.map((part) => (
                  <li
                    key={`${part.folderName}-${part.file}-${part.section}-${part.line}`}
                    className="text-[11px] leading-relaxed text-zinc-400"
                  >
                    <span className="font-display tracking-wide text-zinc-200">
                      {part.displayName}
                    </span>
                    <span className="text-zinc-600"> — </span>
                    <span className="font-mono text-[10px] text-zinc-500">
                      {part.file} · {part.section}:{part.line}
                    </span>
                  </li>
                ))}
              </ul>
            </details>
          );
        })}
      </div>
    </Panel>
  );
}
