import { create } from "zustand";

import { api } from "@/lib/api";
import { toUserMessage } from "@/lib/errors";
import type {
  BrowseSort,
  CategoryChip,
  ModDetail,
  ModSummary,
} from "@/types/gamebanana";

export type BrowseView = "grid" | "list";

const VIEW_STORAGE_KEY = "zzmm.browseView";

function readStoredView(): BrowseView {
  try {
    return localStorage.getItem(VIEW_STORAGE_KEY) === "list" ? "list" : "grid";
  } catch {
    return "grid";
  }
}

export interface BrowseFilters {
  search: string;
  sort: BrowseSort;
  categoryId: number | null;
  character: string;
}

interface BrowseStore {
  filters: BrowseFilters;
  view: BrowseView;
  categories: CategoryChip[];
  categoryError: string | null;

  items: ModSummary[];
  cursor: number | null;
  hasMore: boolean;
  total: number;
  lastCached: boolean;
  isLoading: boolean;
  isLoadingMore: boolean;
  error: string | null;

  details: Record<number, ModDetail>;
  detailLoadingId: number | null;
  detailError: string | null;

  setSearch: (value: string) => void;
  setSort: (value: BrowseSort) => void;
  setCategory: (id: number | null) => void;
  setCharacter: (value: string) => void;
  setView: (view: BrowseView) => void;
  refresh: () => void;
  loadMore: () => void;
  loadCategories: () => Promise<void>;
  loadDetail: (modId: number) => Promise<void>;
}

/** Guards against out-of-order responses when filters change mid-request. */
let requestSeq = 0;

function mergeUnique(existing: ModSummary[], incoming: ModSummary[]): ModSummary[] {
  const seen = new Set(existing.map((item) => item.id));
  return [...existing, ...incoming.filter((item) => !seen.has(item.id))];
}

export const useBrowseStore = create<BrowseStore>((set, get) => {
  const fetchPage = async (append: boolean) => {
    const { filters, cursor, categories } = get();
    const seq = ++requestSeq;

    set(
      append
        ? { isLoadingMore: true, error: null }
        : {
            isLoading: true,
            isLoadingMore: false,
            error: null,
            items: [],
            cursor: null,
            hasMore: true,
          },
    );

    try {
      const page = await api.gamebanana.browse({
        cursor: append ? cursor : null,
        search: filters.search.trim() || null,
        sort: filters.sort,
        categoryId: filters.categoryId,
        categoryName: filters.categoryId
          ? (categories.find((category) => category.id === filters.categoryId)?.name ??
            null)
          : null,
        character: filters.character.trim() || null,
      });

      // A newer request superseded this one.
      if (seq !== requestSeq) return;

      set((state) => ({
        items: append ? mergeUnique(state.items, page.items) : page.items,
        cursor: page.nextCursor,
        hasMore: page.hasMore,
        total: page.total,
        lastCached: page.cached,
        isLoading: false,
        isLoadingMore: false,
      }));
    } catch (error) {
      if (seq !== requestSeq) return;
      set({
        isLoading: false,
        isLoadingMore: false,
        error: toUserMessage(error),
      });
    }
  };

  const applyFilters = (patch: Partial<BrowseFilters>) => {
    set((state) => ({ filters: { ...state.filters, ...patch } }));
    void fetchPage(false);
  };

  return {
    filters: { search: "", sort: "newest", categoryId: null, character: "" },
    view: readStoredView(),
    categories: [],
    categoryError: null,

    items: [],
    cursor: null,
    hasMore: true,
    total: 0,
    lastCached: false,
    isLoading: false,
    isLoadingMore: false,
    error: null,

    details: {},
    detailLoadingId: null,
    detailError: null,

    setSearch: (value) => {
      if (get().filters.search === value) return;
      applyFilters({ search: value });
    },

    setSort: (value) => {
      if (get().filters.sort === value) return;
      applyFilters({ sort: value });
    },

    setCategory: (id) => {
      if (get().filters.categoryId === id) return;
      applyFilters({ categoryId: id });
    },

    setCharacter: (value) => {
      if (get().filters.character === value) return;
      applyFilters({ character: value });
    },

    setView: (view) => {
      if (get().view === view) return;
      set({ view });
      try {
        localStorage.setItem(VIEW_STORAGE_KEY, view);
      } catch {
        // A blocked localStorage only costs us the remembered preference.
      }
    },

    refresh: () => {
      void fetchPage(false);
    },

    loadMore: () => {
      const state = get();
      if (!state.hasMore || state.isLoading || state.isLoadingMore) return;
      void fetchPage(true);
    },

    loadCategories: async () => {
      if (get().categories.length > 0) return;

      try {
        const categories = await api.gamebanana.categories();
        set({ categories, categoryError: null });
      } catch (error) {
        set({ categoryError: toUserMessage(error) });
      }
    },

    loadDetail: async (modId) => {
      if (get().details[modId]) return;

      set({ detailLoadingId: modId, detailError: null });

      try {
        const detail = await api.gamebanana.detail(modId);
        set((state) => ({
          details: { ...state.details, [detail.id]: detail },
          detailLoadingId: null,
        }));
      } catch (error) {
        set({ detailLoadingId: null, detailError: toUserMessage(error) });
      }
    },
  };
});
