import { invoke } from "@tauri-apps/api/core";

/** Error shape produced by `AppError`'s Serialize impl on the Rust side. */
interface SerializedAppError {
  code?: unknown;
  message?: unknown;
}

export class ApiError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }
}

function hasTauriBridge(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function normalizeError(error: unknown): ApiError {
  if (error instanceof ApiError) return error;

  if (typeof error === "object" && error !== null) {
    const value = error as SerializedAppError;
    if (typeof value.message === "string" && value.message.length > 0) {
      return new ApiError(
        typeof value.code === "string" ? value.code : "unknown",
        value.message,
      );
    }
  }

  if (typeof error === "string" && error.length > 0) {
    return new ApiError("unknown", error);
  }

  if (error instanceof Error) {
    return new ApiError("unknown", error.message);
  }

  return new ApiError("unknown", "An unexpected error occurred.");
}

/**
 * Single entry point for every Tauri command. Normalizes backend errors into
 * `ApiError` so the UI can show friendly toasts without inspecting payloads.
 */
export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!hasTauriBridge()) {
    throw new ApiError(
      "bridge_unavailable",
      "ZZMM's desktop backend is not available. Start the app with `pnpm tauri dev`.",
    );
  }

  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeError(error);
  }
}
