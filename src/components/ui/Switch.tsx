import { cn } from "@/lib/cn";

interface SwitchProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  label: string;
}

/** Pill switch; fills acid-green when the mod is enabled. */
export function Switch({ checked, onCheckedChange, disabled, label }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onCheckedChange(!checked)}
      className={cn(
        "relative h-5 w-9 shrink-0 rounded-full border transition-colors duration-150 ease-smooth disabled:pointer-events-none disabled:opacity-40",
        checked
          ? "border-acid-400 bg-acid-400"
          : "border-ink-600 bg-ink-800",
      )}
    >
      <span
        aria-hidden
        className={cn(
          "absolute top-1/2 h-3.5 w-3.5 -translate-y-1/2 rounded-full transition-[left,background-color] duration-150 ease-smooth",
          checked
            ? "left-[18px] bg-ink-950"
            : "left-[3px] bg-zinc-500",
        )}
      />
    </button>
  );
}
