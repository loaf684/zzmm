import { useState } from "react";
import { UserRound } from "lucide-react";

import { Button } from "@/components/ui/Button";
import { SuggestionInput } from "@/components/ui/SuggestionInput";

interface CharacterPromptProps {
  /** Folder names created by the last install batch. */
  folders: string[];
  /** Characters already used in the library, offered as suggestions. */
  suggestions: string[];
  busy: boolean;
  onSkip: () => void;
  onAssign: (character: string) => void | Promise<void>;
}

/**
 * Asks which character a freshly installed mod belongs to. ZZMI itself has no
 * concept of characters, so this only writes ZZMM's own `.zzmm.json`; skipping
 * keeps the automatic name-based guess.
 */
export function CharacterPrompt({
  folders,
  suggestions,
  busy,
  onSkip,
  onAssign,
}: CharacterPromptProps) {
  const [value, setValue] = useState("");
  const trimmed = value.trim();

  return (
    <div
      className="fixed inset-0 z-[65] flex items-center justify-center bg-ink-950/75 p-6 backdrop-blur-sm"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onSkip();
      }}
    >
      <form
        className="w-full max-w-md animate-fade-in rounded-2xl border border-ink-700 bg-ink-950 p-5 shadow-pop"
        onSubmit={(event) => {
          event.preventDefault();
          if (trimmed && !busy) void onAssign(trimmed);
        }}
      >
        <div className="flex items-start gap-3">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-ink-700 bg-ink-900 text-acid-300">
            <UserRound className="h-4 w-4" aria-hidden />
          </span>
          <div className="min-w-0">
            <h2 className="font-display text-[15px] font-semibold text-zinc-50">
              Which character is this for?
            </h2>
            <p className="mt-1 text-[11px] leading-relaxed text-zinc-500">
              {folders.length === 1
                ? "The new folder will be"
                : `All ${folders.length} new folders will be`}{" "}
              tagged, so character filters and one-active-per-character work
              right away. Skip to keep the automatic guess.
            </p>
          </div>
        </div>

        <ul className="mt-3 max-h-28 space-y-0.5 overflow-y-auto rounded-lg border border-ink-800 bg-ink-900/50 p-2">
          {folders.map((folder) => (
            <li
              key={folder}
              className="truncate font-mono text-[10px] leading-relaxed text-zinc-500"
              title={folder}
            >
              {folder}
            </li>
          ))}
        </ul>

        <SuggestionInput
          value={value}
          onChange={setValue}
          suggestions={suggestions}
          maxLength={80}
          placeholder="e.g. Ellen Joe"
          ariaLabel="Character name"
          autoFocus
          disabled={busy}
          className="mt-3"
          inputClassName="h-9 rounded-lg border-ink-700 bg-ink-900 px-3 text-[12px]"
        />

        <div className="mt-4 flex items-center justify-between gap-2">
          <Button variant="ghost" onClick={onSkip} disabled={busy}>
            Skip
          </Button>
          <Button
            type="submit"
            variant="primary"
            loading={busy}
            disabled={!trimmed}
          >
            Assign character
          </Button>
        </div>
      </form>
    </div>
  );
}
