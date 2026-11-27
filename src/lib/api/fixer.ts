import { call } from "@/lib/api/client";
import type {
  FixerApplyResult,
  FixerRollbackResult,
  FixerScanResult,
} from "@/types/fixer";

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
};