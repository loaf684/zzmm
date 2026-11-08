import { toast } from "@/store/toasts";

let lastReminder = 0;
const THROTTLE_MS = 1_500;

/**
 * 3DMigoto only picks up folder/INI changes after an in-game reload. Batch
 * operations can finish several actions at once, so reminders are throttled to
 * avoid stacking identical toasts.
 */
export function remindReload(
  message = "Press F10 in-game to reload your mods.",
): void {
  const now = Date.now();
  if (now - lastReminder < THROTTLE_MS) return;
  lastReminder = now;

  toast.info("Reload mods in game", message);
}
