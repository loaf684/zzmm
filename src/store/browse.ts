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

  /** The storefront's Trending feed: fetched once with the trending sort and
   *  never touched by the result sort, filters or paging. */
  trending: ModSummary[];
  trendingLoading: boolean;
  trendingError: string | null;

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
  loadTrending: (force?: boolean) => Promise<void>;
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
  const fetchPage = async (append: boolean, keepResults = false) => {
    const { filters, cursor } = get();
    const seq = ++requestSeq;

    set(
      append
        ? { isLoadingMore: true, error: null }
        : {
            isLoading: true,
            isLoadingMore: false,
            error: null,
            // A sort change reloads the same storefront, so the previous page
            // stays on screen until the new one lands; only a new query (search,
            // category, character) drops it.
            ...(keepResults
              ? {}
              : { items: [] as ModSummary[], cursor: null, hasMore: true }),
          },
    );

    // GameBanana filters categories by name, and it only does so server-side on
    // the index; the subfeed sorts match the name client-side. Resolve the id
    // against the category list before asking, so a category click cannot race
    // the category fetch and come back unfiltered.
    if (
      filters.categoryId !== null &&
      !get().categories.some((category) => category.id === filters.categoryId)
    ) {
      await get().loadCategories();
      if (seq !== requestSeq) return;
    }

    try {
      const categories = get().categories;
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
    // Only the order changed: the shelf, hero and grid stay put while it loads.
    const keepsResults = Object.keys(patch).every((key) => key === "sort");
    void fetchPage(false, keepsResults);
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

    trending: [],
    trendingLoading: false,
    trendingError: null,

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
      // Same query as before, so keep what is rendered while it reloads.
      void fetchPage(false, true);
    },

    loadMore: () => {
      const state = get();
      if (!state.hasMore || state.isLoading || state.isLoadingMore) return;
      void fetchPage(true, true);
    },

    loadTrending: async (force = false) => {
      const state = get();
      if (!force && (state.trending.length > 0 || state.trendingLoading)) return;

      set({ trendingLoading: true, trendingError: null });

      try {
        // Unfiltered on purpose: the rails are the game-wide trending feed and
        // must not follow the result sort, filters or cursor.
        const page = await api.gamebanana.browse({ sort: "trending" });
        set({ trending: page.items, trendingLoading: false });
      } catch (error) {
        set({ trendingLoading: false, trendingError: toUserMessage(error) });
      }
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
