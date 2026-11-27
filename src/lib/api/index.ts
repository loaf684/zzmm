import { fixerApi } from "@/lib/api/fixer";
import { gamebananaApi } from "@/lib/api/gamebanana";
import { iniApi } from "@/lib/api/ini";
import { installerApi } from "@/lib/api/installer";
import { modsApi } from "@/lib/api/mods";
import { previewApi } from "@/lib/api/preview";
import { settingsApi } from "@/lib/api/settings";

/** Feature-grouped API surface used by all React code. */
export const api = {
  settings: settingsApi,
  mods: modsApi,
  installer: installerApi,
  gamebanana: gamebananaApi,
  fixer: fixerApi,
  ini: iniApi,
  preview: previewApi,
};

export { ApiError } from "@/lib/api/client";
