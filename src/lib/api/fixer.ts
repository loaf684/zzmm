import { call } from "@/lib/api/client";
import type {
  FixerApplyResult,
  FixerScanResult,
  FixerStateView,
  FixerUndoResult,
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

  /**
   * Rewrite the named inis. With `snapshot`, each written mod's single undo
   * snapshot becomes the state this run overwrote; without it, a stale snapshot
   * for a written mod is dropped.
   */
  apply(
    modsDir: string,
    iniPaths: string[],
    snapshot: boolean,
  ): Promise<FixerApplyResult> {
    return call<FixerApplyResult>("fixer_apply", { modsDir, iniPaths, snapshot });
  },

  /** The persisted ignore list and the mods an undo can still put back. */
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
   * Put one mod back from its undo snapshot: the state right before its most
   * recent fix. The stored originals live in the backend, never in the caller.
   */
  undo(modsDir: string, folder: string): Promise<FixerUndoResult> {
    return call<FixerUndoResult>("fixer_undo", { modsDir, folder });
  },
};