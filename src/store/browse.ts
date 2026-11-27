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

/** One trending shelf: the dashboard's own trending feed, grouped by the root
 *  category its cards actually carry, so a shelf label never lies. */
export interface BrowseShelf {
  /** Null for the "Trending now" catch-all, which has no category to widen to. */
  categoryId: number | null;
  /** Empty renders the generic "Trending now" heading. */
  label: string;
  items: ModSummary[];
}

/** Named category shelves, cards kept per shelf, and feed pages walked. */
const SHELF_CATEGORIES = 3;
const SHELF_LIMIT = 12;
const SHELF_PAGES = 2;
/** A row this thin is not worth its heading. */
const SHELF_MIN_ITEMS = 3;

interface BrowseStore {
  filters: BrowseFilters;
  view: BrowseView;
  categories: CategoryChip[];
  categoryError: string | null;

  rails: BrowseShelf[];
  railsLoading: boolean;
  railsError: string | null;

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
  /** Real `sort=trending` shelves, independent of the result feed. */
  loadRails: (force?: boolean) => Promise<void>;
  /** Featured hero detail (description, gallery, downloads); failures stay quiet. */
  loadSpotlightDetail: (modId: number) => Promise<void>;
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
      // A failed reset cannot keep the previous filter's rows: the page would
      // show mods the current filters exclude. An appended page keeps its base.
      set(
        append
          ? { isLoadingMore: false, error: toUserMessage(error) }
          : {
              isLoading: false,
              isLoadingMore: false,
              error: toUserMessage(error),
              items: [],
              cursor: null,
              hasMore: false,
            },
      );
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

    rails: [],
    railsLoading: false,
    railsError: null,

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
      // The shelves are their own feed, so the header button refreshes them too.
      void get().loadRails(true);
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

    loadRails: async (force = false) => {
      const state = get();
      if (state.railsLoading) return;
      if (state.rails.length > 0 && !force) return;

      set({ railsLoading: true, railsError: null });

      // Shelf headings are real category names, so the list has to settle first
      // (the page loads it in parallel; either call can win).
      if (get().categories.length === 0) await get().loadCategories();

      // The dashboard's own trending feed: never the result page, so sorting and
      // "Load more" cannot relabel or reshuffle the shelves under the pointer.
      const pages = await Promise.allSettled(
        Array.from({ length: SHELF_PAGES }, (_, index) =>
          api.gamebanana.browse({ cursor: index + 1, sort: "trending" }),
        ),
      );

      const feed: ModSummary[] = [];
      const feedIds = new Set<number>();

      for (const page of pages) {
        if (page.status !== "fulfilled") continue;
        for (const mod of page.value.items) {
          if (feedIds.has(mod.id)) continue;
          feedIds.add(mod.id);
          feed.push(mod);
        }
      }

      const failure = pages.find(
        (page): page is PromiseRejectedResult => page.status === "rejected",
      );

      if (feed.length === 0) {
        set({
          rails: [],
          railsLoading: false,
          railsError: failure ? toUserMessage(failure.reason) : null,
        });
        return;
      }

      // Group by each mod's own root category: a "Trending in X" row only ever
      // holds X's cards, and the feed's engagement order decides the rest.
      const groups = new Map<string, ModSummary[]>();

      for (const mod of feed) {
        const name = mod.category?.trim();
        if (!name) continue;

        const key = name.toLowerCase();
        const list = groups.get(key);
        if (list) list.push(mod);
        else groups.set(key, [mod]);
      }

      const named = [...groups.entries()]
        .filter(([, items]) => items.length >= SHELF_MIN_ITEMS)
        .sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]))
        .slice(0, SHELF_CATEGORIES);

      const categories = get().categories;
      const shelves: BrowseShelf[] = [];

      for (const [key, items] of named) {
        shelves.push({
          categoryId:
            categories.find((entry) => entry.name.toLowerCase() === key)?.id ??
            null,
          label: items[0].category?.trim() ?? key,
          items: items.slice(0, SHELF_LIMIT),
        });
      }

      // Whatever has no named row (uncategorized mods, or a group that missed
      // the cut) still trends: one honest catch-all row, also the fallback when
      // the feed carries no usable categories at all.
      const shelved = new Set(named.map(([key]) => key));
      const rest = feed.filter(
        (mod) => !mod.category || !shelved.has(mod.category.trim().toLowerCase()),
      );
      if (rest.length >= SHELF_MIN_ITEMS) {
        shelves.push({
          categoryId: null,
          label: "",
          items: rest.slice(0, SHELF_LIMIT),
        });
      }

      set({
        rails: shelves,
        railsLoading: false,
        railsError:
          shelves.length === 0 && failure ? toUserMessage(failure.reason) : null,
      });
    },

    loadSpotlightDetail: async (modId) => {
      if (get().details[modId]) return;

      try {
        const detail = await api.gamebanana.detail(modId);
        set((state) => ({
          details: { ...state.details, [detail.id]: detail },
        }));
      } catch {
        // The summary alone still renders the hero; leave detailError alone.
      }
    },
  };
});
