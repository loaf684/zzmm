import { call } from "@/lib/api/client";
import type { FixApplyResult, FixRestoreResult, FixScan } from "@/types/fixer";

export const fixerApi = {
  /** Read-only check for outdated `hash = ...` values. Never writes. */
  scan(folderName: string): Promise<FixScan> {
    return call<FixScan>("scan_mod_hash_fixes", { folderName });
  },

  /** Rewrite outdated values, keeping `<file>.ini.bak` per changed INI. */
  apply(folderName: string): Promise<FixApplyResult> {
    return call<FixApplyResult>("apply_mod_hash_fixes", { folderName });
  },

  /** Restore every INI that still has a `.ini.bak`. */
  restoreBackups(folderName: string): Promise<FixRestoreResult> {
    return call<FixRestoreResult>("restore_mod_ini_backups", { folderName });
  },
};