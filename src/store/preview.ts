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
  /**
   * Explicit `$variable` choices from the preview controls. Sent with every
   * build so a rebuild keeps the variant the user picked.
   */
  toggleValues: Record<string, string>;

  load: (folderName: string) => Promise<void>;
  reload: () => Promise<void>;
  setToggle: (name: string, value: string) => Promise<void>;
  resetToggles: () => Promise<void>;
  togglePart: (name: string) => void;
  setAllPartsVisible: (visible: boolean) => void;
  setWireframe: (wireframe: boolean) => void;
}

export const usePreviewStore = create<PreviewStore>((set, get) => {
  // A newer build supersedes any older one still decoding; only the latest
  // response may land, so quick toggle clicks never show a stale variant.
  let request = 0;

  const build = async (
    folderName: string,
    toggleValues: Record<string, string>,
    keepPreview: boolean,
  ) => {
    const ticket = ++request;
    // Restore the last accepted choice if a rebuild fails, so the controls and
    // the kept mesh stay in agreement.
    const previousToggles = keepPreview ? get().toggleValues : {};
    set({
      modFolder: folderName,
      isLoading: true,
      error: null,
      toggleValues,
      ...(keepPreview ? {} : { preview: null }),
    });

    try {
      const preview = await api.preview.load(folderName, toggleValues);
      if (ticket !== request || get().modFolder !== folderName) return;
      // A toggle change can add or drop parts: prune hidden names that the new
      // mesh no longer contains so a stale entry never hides a future part.
      const visible = new Set(preview.parts.map((part) => part.name));
      set((state) => ({
        preview,
        isLoading: false,
        hiddenParts: state.hiddenParts.filter((name) => visible.has(name)),
      }));
    } catch (error) {
      if (ticket !== request || get().modFolder !== folderName) return;
      // A rebuild keeps the last good mesh so its controls stay usable; the
      // error is surfaced next to them instead of replacing the viewer.
      set({
        isLoading: false,
        error: toUserMessage(error),
        toggleValues: previousToggles,
      });
    }
  };

  return {
    modFolder: null,
    preview: null,
    isLoading: false,
    error: null,

    hiddenParts: [],
    wireframe: false,
    toggleValues: {},

    load: async (folderName) => {
      // A different mod starts from its own saved in-game toggle state and a
      // clean per-part visibility selection.
      set({ hiddenParts: [] });
      await build(folderName, {}, false);
    },

    reload: async () => {
      const folder = get().modFolder;
      if (folder) await build(folder, get().toggleValues, true);
    },

    setToggle: async (name, value) => {
      const folder = get().modFolder;
      if (!folder) return;
      await build(folder, { ...get().toggleValues, [name]: value }, true);
    },

    resetToggles: async () => {
      const folder = get().modFolder;
      if (folder) await build(folder, {}, true);
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
  };
});