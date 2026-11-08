import type { ButtonHTMLAttributes } from "react";
import { forwardRef } from "react";
import { Loader2 } from "lucide-react";

import { cn } from "@/lib/cn";

type Variant = "primary" | "outline" | "ghost" | "danger";
type Size = "sm" | "md";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
}

const VARIANTS: Record<Variant, string> = {
  primary:
    "border border-acid-400 bg-acid-400 text-ink-950 hover:border-acid-300 hover:bg-acid-300",
  outline:
    "border border-ink-700 bg-ink-900 text-zinc-300 hover:border-ink-500 hover:bg-ink-800 hover:text-zinc-100",
  ghost:
    "border border-transparent text-zinc-400 hover:bg-ink-800 hover:text-zinc-100",
  danger:
    "border border-hot-500/40 bg-hot-500/10 text-hot-400 hover:bg-hot-500/20",
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    {
      className,
      variant = "outline",
      size = "md",
      loading = false,
      disabled,
      type = "button",
      children,
      ...props
    },
    ref,
  ) {
    return (
      <button
        ref={ref}
        type={type}
        disabled={disabled || loading}
        className={cn(
          "inline-flex shrink-0 items-center justify-center gap-1.5 rounded-lg text-[12px] font-medium transition-colors duration-150 ease-smooth disabled:pointer-events-none disabled:opacity-40",
          size === "sm" ? "h-8 px-3 text-[11px]" : "h-9 px-3.5",
          VARIANTS[variant],
          className,
        )}
        {...props}
      >
        {loading ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden />
        ) : null}
        {children}
      </button>
    );
  },
);
