export type InstallStage =
  | "extracting"
  | "detecting"
  | "installing"
  | "done"
  | "failed"
  | "cancelled";

export interface InstallProgress {
  archive: string;
  stage: InstallStage;
  percent: number;
  message: string;
}

export interface ArchiveInstallOutcome {
  archive: string;
  installed: string[];
  warnings: string[];
  error: string | null;
  cancelled: boolean;
}

export interface InstallBatchResult {
  outcomes: ArchiveInstallOutcome[];
}
