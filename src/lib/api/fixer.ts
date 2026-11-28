import { call } from "@/lib/api/client";
import type {
  FixerApplyResult,
  FixerRollbackResult,
  FixerScanResult,
  FixerStateView,
} from "@/types/fixer";

/** One entry to add to or remove from the persisted ignore list. */
export interface FixerIgnoreTarget {
  /** Mod folder: the first path component under the Mods folder. */
  folder: string;
  /** Ini path, for a file- or fix-scoped entry. */
  iniPath?: string | null;
  /** Recommendation key, for a fix-scoped entry. */
  key?: string | null;
  /** Display text for the ignore list. */
  label: string;
}

export const fixerApi = {
  /** Read-only: reports what the fixer database would change, writes nothing. */
  scan(modsDir: string): Promise<FixerScanResult> {
    return call<FixerScanResult>("fixer_scan", { modsDir });
  },

  apply(
    modsDir: string,
    iniPaths: string[],
    backup: boolean,
  ): Promise<FixerApplyResult> {
    return call<FixerApplyResult>("fixer_apply", { modsDir, iniPaths, backup });
  },

  rollback(modsDir: string, iniPaths: string[]): Promise<FixerRollbackResult> {
    return call<FixerRollbackResult>("fixer_rollback", { modsDir, iniPaths });
  },

  /** The persisted ignore list and fix history. */
  state(): Promise<FixerStateView> {
    return call<FixerStateView>("fixer_state");
  },

  /**
   * Ignore or unignore a mod, an ini, or a single recommendation. Writes the
   * fixer state file only - a mod's ini is never touched by this.
   */
  setIgnored(
    target: FixerIgnoreTarget,
    ignored: boolean,
  ): Promise<FixerStateView> {
    return call<FixerStateView>("fixer_ignore", {
      folder: target.folder,
      iniPath: target.iniPath ?? null,
      key: target.key ?? null,
      label: target.label,
      ignored,
    });
  },

  /**
   * Put the original ini back from the backup one recorded fix wrote. The
   * backup path lives in the stored history, so this can only restore files the
   * fixer itself recorded.
   */
  restore(
    modsDir: string,
    iniPath: string,
    fixedAt: number,
  ): Promise<FixerRollbackResult> {
    return call<FixerRollbackResult>("fixer_restore", {
      modsDir,
      iniPath,
      fixedAt,
    });
  },
};
