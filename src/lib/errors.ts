import { ApiError } from "@/lib/api/client";

/** Convert any thrown value into a message that is safe to show in a toast. */
export function toUserMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim().length > 0) return error;
  return "An unexpected error occurred.";
}
