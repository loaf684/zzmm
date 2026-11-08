import { call } from "@/lib/api/client";
import type { MeshPreview } from "@/types/preview";

export const previewApi = {
  load(folderName: string): Promise<MeshPreview> {
    return call<MeshPreview>("load_mod_preview", { folderName });
  },
};
