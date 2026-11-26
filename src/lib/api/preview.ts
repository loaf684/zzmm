import { call } from "@/lib/api/client";
import type { MeshPreview, PreviewToggleSave } from "@/types/preview";

export const previewApi = {
  /**
   * Build the preview for one mod. `toggles` are the viewer's explicit
   * `$variable` choices; the backend ignores names the mod does not gate on.
   */
  load(
    folderName: string,
    toggles?: Record<string, string>,
  ): Promise<MeshPreview> {
    const overrides =
      toggles && Object.keys(toggles).length > 0
        ? Object.entries(toggles).map(([name, value]) => ({ name, value }))
        : null;

    return call<MeshPreview>("load_mod_preview", {
      folderName,
      toggles: overrides,
    });
  },

  /**
   * Persist the viewer's choices where the loader reads them (`d3dx_user.ini`
   * under the declaring INI's namespace), so the game starts with the same
   * outfit the preview shows. Only toggles the mod declares are written; the
   * rest come back in `missing`.
   */
  save(
    folderName: string,
    values: Record<string, string>,
  ): Promise<PreviewToggleSave> {
    const list = Object.entries(values).map(([name, value]) => ({ name, value }));
    return call<PreviewToggleSave>("save_preview_toggles", {
      folderName,
      values: list,
    });
  },
};