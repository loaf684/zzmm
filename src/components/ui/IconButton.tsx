import type { ButtonHTMLAttributes, ReactNode } from "react";
import { forwardRef } from "react";

import { cn } from "@/lib/cn";

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  tone?: "default" | "danger";
  children: ReactNode;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
  function IconButton(
    { label, tone = "default", className, type = "button", children, ...props },
    ref,
  ) {
    return (
      <button
        ref={ref}
        type={type}
        title={label}
        aria-label={label}
        className={cn(
          "inline-flex h-7 w-7 items-center justify-center rounded-md text-zinc-500 transition-colors duration-150 ease-smooth hover:bg-ink-800 hover:text-zinc-200 disabled:pointer-events-none disabled:opacity-40",
          tone === "danger" && "hover:bg-hot-500/10 hover:text-hot-400",
          className,
        )}
        {...props}
      >
        {children}
      </button>
    );
  },
);
