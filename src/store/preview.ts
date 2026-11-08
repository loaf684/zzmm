import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import type { MeshPreview } from "@/types/preview";

interface PreviewStore {
  modFolder: string | null;
  preview: MeshPreview | null;
  isLoading: boolean;
  error: string | null;

  hiddenParts: string[];
  wireframe: boolean;

  load: (folderName: string) => Promise<void>;
  reload: () => Promise<void>;
  togglePart: (name: string) => void;
  setAllPartsVisible: (visible: boolean) => void;
  setWireframe: (wireframe: boolean) => void;
}

export const usePreviewStore = create<PreviewStore>((set, get) => ({
  modFolder: null,
  preview: null,
  isLoading: false,
  error: null,

  hiddenParts: [],
  wireframe: false,

  load: async (folderName) => {
    // Never run two preview builds at once; the last one wins.
    set({
      modFolder: folderName,
      isLoading: true,
      error: null,
      preview: null,
      hiddenParts: [],
    });

    try {
      const preview = await api.preview.load(folderName);
      // A different mod was requested while this one was decoding.
      if (get().modFolder !== folderName) return;
      set({ preview, isLoading: false });
    } catch (error) {
      if (get().modFolder !== folderName) return;
      set({ isLoading: false, error: toUserMessage(error) });
    }
  },

  reload: async () => {
    const folder = get().modFolder;
    if (folder) await get().load(folder);
  },

  togglePart: (name) =>
    set((state) => ({
      hiddenParts: state.hiddenParts.includes(name)
        ? state.hiddenParts.filter((entry) => entry !== name)
        : [...state.hiddenParts, name],
    })),

  setAllPartsVisible: (visible) =>
    set((state) => ({
      hiddenParts: visible
        ? []
        : (state.preview?.parts.map((part) => part.name) ?? []),
    })),

  setWireframe: (wireframe) => set({ wireframe }),
}));
