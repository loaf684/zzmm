import { useEffect, useId, useMemo, useRef, useState } from "react";
import { ChevronDown } from "lucide-react";

import { cn } from "@/lib/cn";

interface SuggestionInputProps {
  value: string;
  onChange: (value: string) => void;
  suggestions: string[];
  placeholder?: string;
  ariaLabel: string;
  autoFocus?: boolean;
  maxLength?: number;
  disabled?: boolean;
  /** Wrapper classes (the wrapper is `relative`, the menu anchors to it). */
  className?: string;
  /** Classes merged onto the `<input>` itself. */
  inputClassName?: string;
}

/**
 * A themed replacement for `<input list=…>`/`<datalist>`.
 *
 * The native datalist popup is drawn by Chromium and cannot be styled: in the
 * Tauri webview it shows up as a white system menu with a white scrollbar that
 * clashes with the dark UI. This control keeps the same behaviour (free text
 * plus filtered suggestions, arrow keys, Enter, Escape) but renders the list
 * itself.
 */
export function SuggestionInput({
  value,
  onChange,
  suggestions,
  placeholder,
  ariaLabel,
  autoFocus,
  maxLength,
  disabled,
  className,
  inputClassName,
}: SuggestionInputProps) {
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(-1);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const listId = useId();

  const matches = useMemo(() => {
    const query = value.trim().toLowerCase();
    const pool = query
      ? suggestions.filter((item) => item.toLowerCase().includes(query))
      : suggestions;
    return pool.slice(0, 40);
  }, [suggestions, value]);

  useEffect(() => {
    if (!open) return;

    const onPointerDown = (event: MouseEvent) => {
      if (!wrapperRef.current?.contains(event.target as Node)) setOpen(false);
    };

    window.addEventListener("mousedown", onPointerDown);
    return () => window.removeEventListener("mousedown", onPointerDown);
  }, [open]);

  useEffect(() => {
    setHighlight(-1);
  }, [value]);

  const choose = (next: string) => {
    onChange(next);
    setOpen(false);
    setHighlight(-1);
  };

  return (
    <div ref={wrapperRef} className={cn("relative", className)}>
      <input
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => {
          // Let a click on an option land before the list disappears.
          window.setTimeout(() => setOpen(false), 120);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setOpen(true);
            setHighlight((current) => Math.min(current + 1, matches.length - 1));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setHighlight((current) => Math.max(current - 1, -1));
          } else if (event.key === "Enter") {
            if (open && highlight >= 0 && matches[highlight]) {
              event.preventDefault();
              choose(matches[highlight]);
            } else {
              setOpen(false);
            }
          } else if (event.key === "Escape" && open) {
            event.stopPropagation();
            setOpen(false);
          }
        }}
        placeholder={placeholder}
        aria-label={ariaLabel}
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
        autoFocus={autoFocus}
        maxLength={maxLength}
        disabled={disabled}
        autoComplete="off"
        className={cn(
          "h-10 w-full border border-ink-600 bg-ink-900/70 pl-3 pr-8 text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-acid-400/60 focus:outline-none disabled:opacity-50",
          inputClassName,
        )}
      />

      <button
        type="button"
        tabIndex={-1}
        aria-label={open ? "Hide suggestions" : "Show suggestions"}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => setOpen((current) => !current)}
        className="absolute right-1 top-1/2 flex h-6 w-6 -translate-y-1/2 items-center justify-center text-zinc-500 transition-colors hover:text-zinc-200"
      >
        <ChevronDown
          className={cn("h-3.5 w-3.5 transition-transform", open && "rotate-180")}
          aria-hidden
        />
      </button>

      {open && matches.length > 0 ? (
        <ul
          id={listId}
          role="listbox"
          className="absolute left-0 top-[calc(100%+4px)] z-30 max-h-56 w-full min-w-[200px] overflow-y-auto rounded-lg border border-ink-600 bg-ink-900 py-1 shadow-pop"
        >
          {matches.map((item, index) => (
            <li key={item}>
              <button
                type="button"
                role="option"
                aria-selected={index === highlight}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => choose(item)}
                onMouseEnter={() => setHighlight(index)}
                className={cn(
                  "block w-full truncate px-3 py-1.5 text-left text-[11px] transition-colors",
                  index === highlight
                    ? "bg-acid-400/15 text-acid-200"
                    : "text-zinc-300 hover:bg-ink-800",
                )}
              >
                {item}
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
