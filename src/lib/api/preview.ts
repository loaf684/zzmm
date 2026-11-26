import { call } from "@/lib/api/client";
import type { MeshPreview } from "@/types/preview";

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
};