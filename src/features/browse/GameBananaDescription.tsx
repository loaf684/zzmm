import { Fragment, useMemo } from "react";

import { parseGameBananaDescription } from "@/lib/gamebananaText";
import { useModsStore } from "@/store/mods";

interface GameBananaDescriptionProps {
  raw: string | null | undefined;
}

/**
 * Render GameBanana's untrusted description without `dangerouslySetInnerHTML`.
 * Links go through the Rust opener so the webview never navigates away.
 */
export function GameBananaDescription({ raw }: GameBananaDescriptionProps) {
  const segments = useMemo(() => parseGameBananaDescription(raw), [raw]);
  const openUrl = useModsStore((state) => state.openUrl);

  return (
    <div className="whitespace-pre-wrap break-words text-[12.5px] leading-6 text-zinc-400">
      {segments.map((segment, index) =>
        segment.type === "text" ? (
          <Fragment key={index}>{segment.text}</Fragment>
        ) : (
          <a
            key={index}
            href={segment.href}
            title={segment.href}
            rel="noreferrer noopener"
            onClick={(event) => {
              event.preventDefault();
              void openUrl(segment.href);
            }}
            className="text-acid-300 underline decoration-acid-400/30 underline-offset-2 transition-colors hover:text-acid-200 hover:decoration-acid-400"
          >
            {segment.text}
          </a>
        ),
      )}
    </div>
  );
}
