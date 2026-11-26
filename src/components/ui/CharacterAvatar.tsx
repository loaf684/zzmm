import { useEffect, useMemo, useState } from "react";

import { characterIcon } from "@/lib/characterIcons";
import { cn } from "@/lib/cn";

interface CharacterAvatarProps {
  character: string;
  active?: boolean;
  /** `sm` for navigation rows, `xs` for dense card metadata. */
  size?: "xs" | "sm";
  className?: string;
}

/** Circular character portrait; falls back to the name's initial. */
export function CharacterAvatar({
  character,
  active = false,
  size = "sm",
  className,
}: CharacterAvatarProps) {
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
        "flex shrink-0 items-center justify-center overflow-hidden rounded-full border font-mono",
        size === "xs" ? "h-4 w-4 text-[8px]" : "h-6 w-6 text-[9px]",
        active
          ? "border-acid-400/50 bg-acid-400/10 text-acid-300"
          : "border-ink-700 bg-ink-900 text-zinc-500",
        className,
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
        (character.trim().slice(0, 1) || "?").toUpperCase()
      )}
    </span>
  );
}