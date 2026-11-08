import { clsx, type ClassValue } from "clsx";

/** Tiny class-name joiner used by every UI primitive. */
export function cn(...inputs: ClassValue[]): string {
  return clsx(inputs);
}
