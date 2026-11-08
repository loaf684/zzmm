import { useEffect, useState } from "react";
import { Keyboard } from "lucide-react";

import { formatHotkeyToken } from "@/features/installed/hotkeyLabels";
import { cn } from "@/lib/cn";

const NAMED_KEYS: Record<string, string> = {
  Space: "VK_SPACE",
  Enter: "VK_RETURN",
  NumpadEnter: "VK_RETURN",
  Tab: "VK_TAB",
  Backspace: "VK_BACK",
  Delete: "VK_DELETE",
  Insert: "VK_INSERT",
  Home: "VK_HOME",
  End: "VK_END",
  PageUp: "VK_PRIOR",
  PageDown: "VK_NEXT",
  CapsLock: "VK_CAPITAL",
  Escape: "VK_ESCAPE",
  ArrowUp: "VK_UP",
  ArrowDown: "VK_DOWN",
  ArrowLeft: "VK_LEFT",
  ArrowRight: "VK_RIGHT",
  NumpadAdd: "VK_ADD",
  NumpadSubtract: "VK_SUBTRACT",
  NumpadMultiply: "VK_MULTIPLY",
  NumpadDivide: "VK_DIVIDE",
  NumpadDecimal: "VK_DECIMAL",
  Minus: "VK_OEM_MINUS",
  Equal: "VK_OEM_PLUS",
  BracketLeft: "VK_OEM_4",
  BracketRight: "VK_OEM_6",
  Backslash: "VK_OEM_5",
  Semicolon: "VK_OEM_1",
  Quote: "VK_OEM_7",
  Comma: "VK_OEM_COMMA",
  Period: "VK_OEM_PERIOD",
  Slash: "VK_OEM_2",
  Backquote: "VK_OEM_3",
};

function isModifierKey(key: string): boolean {
  return ["Control", "Alt", "Shift", "Meta"].includes(key);
}

function modifierTokens(event: KeyboardEvent): string[] {
  const tokens: string[] = [];
  if (event.ctrlKey) tokens.push("CTRL");
  if (event.altKey) tokens.push("ALT");
  if (event.shiftKey) tokens.push("SHIFT");
  return tokens;
}

/** Translate a browser key event into a 3DMigoto key token. */
export function keyToken(event: KeyboardEvent): string | null {
  const code = event.code;

  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];

  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1];

  const numpad = /^Numpad([0-9])$/.exec(code);
  if (numpad) return `VK_NUMPAD${numpad[1]}`;

  const functionKey = /^F([1-9]|1[0-9]|2[0-4])$/.exec(code);
  if (functionKey) return `VK_F${functionKey[1]}`;

  if (NAMED_KEYS[code]) return NAMED_KEYS[code];

  if (event.key.length === 1 && /[a-z0-9]/i.test(event.key)) {
    return event.key.toUpperCase();
  }

  return null;
}

interface KeyCaptureProps {
  value: string;
  /** Whether this row is the one listening for the next key press. */
  capturing: boolean;
  noModifiers?: boolean;
  disabled?: boolean;
  onToggle: () => void;
  onCommit: (binding: string) => void;
}

/**
 * Inline binding chip. Clicking it arms key capture right in the row, so a run
 * of bindings can be changed without opening and closing a per-row editor.
 */
export function KeyCapture({
  value,
  capturing,
  noModifiers = false,
  disabled,
  onToggle,
  onCommit,
}: KeyCaptureProps) {
  const [preview, setPreview] = useState<string | null>(null);

  useEffect(() => {
    if (!capturing) {
      setPreview(null);
      return;
    }

    const handler = (event: KeyboardEvent) => {
      if (isModifierKey(event.key)) {
        setPreview(modifierTokens(event).join(" "));
        return;
      }

      event.preventDefault();
      event.stopPropagation();

      // Plain Escape cancels; Escape with modifiers is a valid binding.
      if (event.key === "Escape" && modifierTokens(event).length === 0) {
        onToggle();
        return;
      }

      const token = keyToken(event);
      if (!token) {
        setPreview("Unsupported key");
        return;
      }

      const tokens = [...modifierTokens(event), token];
      if (noModifiers && tokens.length === 1) tokens.unshift("NO_MODIFIERS");

      onCommit(tokens.join(" "));
    };

    // Capture phase so the editor and page shortcuts never see the key press.
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [capturing, noModifiers, onCommit, onToggle]);

  const tokens = value.split(/\s+/).filter(Boolean);

  return (
    <button
      type="button"
      onClick={onToggle}
      disabled={disabled}
      aria-pressed={capturing}
      aria-label={capturing ? "Listening for a key press" : "Rebind hotkey"}
      title={
        capturing
          ? "Press the new key. Escape cancels."
          : "Click, then press the new key"
      }
      className={cn(
        "inline-flex min-h-7 max-w-full shrink-0 flex-wrap items-center gap-1 rounded-lg border px-2 py-1 text-left transition-colors duration-150 ease-smooth disabled:opacity-50",
        capturing
          ? "border-acid-400/70 bg-acid-400/[0.08]"
          : "border-ink-800 bg-ink-900/70 hover:border-acid-400/50 hover:bg-ink-900",
      )}
    >
      {capturing ? (
        <>
          <Keyboard className="h-3 w-3 shrink-0 text-acid-300" aria-hidden />
          <span className="font-mono text-[10px] text-acid-200">
            {preview || "Press a key…"}
          </span>
        </>
      ) : tokens.length === 0 ? (
        <span className="flex items-center gap-1.5 font-mono text-[10px] text-zinc-600">
          <Keyboard className="h-3 w-3" aria-hidden />
          Set key
        </span>
      ) : (
        tokens.map((token, index) => (
          <span key={`${token}-${index}`} className="flex items-center gap-1">
            {index > 0 ? (
              <span aria-hidden className="text-[9px] text-zinc-700">
                +
              </span>
            ) : null}
            <span
              className={cn(
                "rounded px-1 py-0.5 font-mono text-[10px] leading-4",
                token === "NO_MODIFIERS" || token.startsWith("NO_")
                  ? "bg-hot-500/10 text-hot-400"
                  : token === "CTRL" || token === "ALT" || token === "SHIFT"
                    ? "bg-cyan-400/10 text-cyan-300"
                    : "bg-ink-800/90 text-zinc-200",
              )}
            >
              {formatHotkeyToken(token)}
            </span>
          </span>
        ))
      )}
    </button>
  );
}
