import { useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronDown,
  Compass,
  LayoutGrid,
  List,
  Loader2,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { useNavigate } from "react-router-dom";

import { Button } from "@/components/ui/Button";
import { Notice } from "@/components/ui/Notice";
import { Panel } from "@/components/ui/Panel";
import { SuggestionInput } from "@/components/ui/SuggestionInput";
import { cn } from "@/lib/cn";
import { useBrowseStore } from "@/store/browse";
import { useDownloadStore } from "@/store/download";
import { useInstallerStore } from "@/store/installer";
import { useModsStore } from "@/store/mods";
import { useUpdatesStore } from "@/store/updates";
import type { BrowseSort, NsfwFilter } from "@/types/gamebanana";
import { BrowseCard, BrowseCardSkeleton } from "./BrowseCard";
import { BrowseHero, BrowseHeroSkeleton } from "./BrowseHero";
import type { BrowseStats } from "./BrowseHero";
import { BrowseRail, BrowseRailSkeleton } from "./BrowseRail";

const SORT_GROUPS: { label: string; options: { value: BrowseSort; label: string }[] }[] = [
  {
    label: "Feed",
    options: [
      { value: "newest", label: "Newest" },
      { value: "trending", label: "Trending" },
      { value: "updated", label: "Recently updated" },
    ],
  },
  {
    label: "Popularity",
    options: [
      { value: "likes", label: "Most liked" },
      { value: "views", label: "Most viewed" },
      { value: "downloads", label: "Most downloaded" },
    ],
  },
  {
    label: "Other",
    options: [
      { value: "oldest", label: "Oldest" },
      { value: "name", label: "Name A–Z" },
      { value: "name-desc", label: "Name Z–A" },
    ],
  },
];

/** Content filter. The three labels are deliberately distinct: All keeps every
 *  mod, including ones GameBanana did not classify. */
const NSFW_OPTIONS: { value: NsfwFilter; label: string }[] = [
  { value: "all", label: "All content" },
  { value: "sfw", label: "SFW only" },
  { value: "nsfw", label: "NSFW only" },
];

/** Suggestions for the character field. GameBanana's subcategory names are
 *  matched by substring, so partial names like "Ellen" work too. */
const CHARACTER_SUGGESTIONS = [
  "Ellen Joe",
  "Zhu Yuan",
  "Nicole Demara",
  "Anby Demara",
  "Soldier 0 Anby",
  "Billy Kid",
  "Corin Wickes",
  "Caesar King",
  "Burnice White",
  "Jane Doe",
  "Qingyi",
  "Seth Lowell",
  "Grace Howard",
  "Rina",
  "Von Lycaon",
  "Koleda Belobog",
  "Ben Bigger",
  "Anton Ivanov",
  "Soukaku",
  "Astra Yao",
  "Evelyn Chevalier",
  "Hoshimi Miyabi",
  "Asaba Harumasa",
  "Tsukishiro Yanagi",
  "Lighter",
  "Trigger",
  "Vivian Banshee",
  "Hugo Vlad",
  "Yixuan",
  "Ju Fufu",
  "Alice Thymefield",
  "Pulchra Fellini",
  "Orphie Magus",
  "Lucia",
  "Yidhari",
  "Banyue",
  "Manato",
  "Komano",
  "Seed",
  "Pan Yinhu",
  "Piper Wheel",
  "Lucy",
  "Nekomata",
];

export function BrowsePage() {
  const filters = useBrowseStore((state) => state.filters);
  const items = useBrowseStore((state) => state.items);
  const total = useBrowseStore((state) => state.total);
  const hasMore = useBrowseStore((state) => state.hasMore);
  const isLoading = useBrowseStore((state) => state.isLoading);
  const isLoadingMore = useBrowseStore((state) => state.isLoadingMore);
  const error = useBrowseStore((state) => state.error);
  const lastCached = useBrowseStore((state) => state.lastCached);
  const categories = useBrowseStore((state) => state.categories);
  const categoryError = useBrowseStore((state) => state.categoryError);

  const setSearch = useBrowseStore((state) => state.setSearch);
  const setSort = useBrowseStore((state) => state.setSort);
  const view = useBrowseStore((state) => state.view);
  const setView = useBrowseStore((state) => state.setView);
  const setCategory = useBrowseStore((state) => state.setCategory);
  const setCharacter = useBrowseStore((state) => state.setCharacter);
  const setNsfw = useBrowseStore((state) => state.setNsfw);
  const refresh = useBrowseStore((state) => state.refresh);
  const loadMore = useBrowseStore((state) => state.loadMore);
  const loadCategories = useBrowseStore((state) => state.loadCategories);
  const rails = useBrowseStore((state) => state.rails);
  const railsLoading = useBrowseStore((state) => state.railsLoading);
  const railsError = useBrowseStore((state) => state.railsError);
  const loadRails = useBrowseStore((state) => state.loadRails);
  const loadSpotlightDetail = useBrowseStore((state) => state.loadSpotlightDetail);
  const details = useBrowseStore((state) => state.details);

  const modsResult = useModsStore((state) => state.result);
  const updatesResult = useUpdatesStore((state) => state.result);
  const downloading = useDownloadStore((state) => state.isBusy);
  const installingArchive = useInstallerStore((state) => state.isInstalling);

  const navigate = useNavigate();
  const [searchInput, setSearchInput] = useState(filters.search);
  const [dismissedError, setDismissedError] = useState<string | null>(null);
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const catalogRef = useRef<HTMLDivElement | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  /** Set by "See all": the catalog only settles once the storefront is gone. */
  const pendingCatalogFocus = useRef(false);

  // First load, and category chips for the filter row.
  useEffect(() => {
    void loadCategories();
    if (useBrowseStore.getState().items.length === 0) refresh();
  }, [loadCategories, refresh]);

  // Shelves need the category list; the fallback shelf only needs the API.
  useEffect(() => {
    if (categories.length > 0 || categoryError !== null) void loadRails();
  }, [categories.length, categoryError, loadRails]);

  // Debounce typing so every keystroke does not hit the API.
  useEffect(() => {
    const handle = setTimeout(() => setSearch(searchInput), 350);
    return () => clearTimeout(handle);
  }, [searchInput, setSearch]);

  // Infinite scroll: the sentinel sits after the grid.
  useEffect(() => {
    const node = sentinelRef.current;
    if (!node || !hasMore) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) loadMore();
      },
      { rootMargin: "400px 0px" },
    );

    observer.observe(node);
    return () => observer.disconnect();
  }, [hasMore, loadMore, items.length]);

  const activeFilters = Boolean(
    filters.search || filters.character || filters.categoryId !== null,
  );

  // The unfiltered page opens as a storefront: a trending spotlight, real
  // trending shelves, then the catalog. The shelves come from their own
  // `sort=trending` requests, so the feed's sort and Load more cannot relabel
  // or reshuffle them.
  const unfiltered = !activeFilters;
  const featured = unfiltered
    ? (rails[0]?.items[0] ?? (railsLoading ? null : (items[0] ?? null)))
    : null;
  const featuredId = featured?.id ?? null;
  const featuredDetail = featuredId !== null ? details[featuredId] : undefined;
  /** The hero only exists on the unfiltered dashboard. */
  const spotlight = unfiltered ? featured : null;

  const shelves = unfiltered
    ? rails
        .map((shelf) => ({
          ...shelf,
          items: shelf.items.filter((mod) => mod.id !== featuredId),
        }))
        .filter((shelf) => shelf.items.length > 0)
    : [];

  // The spotlight picks one mod, so it must not be the first grid card too.
  const visibleItems =
    featuredId === null ? items : items.filter((mod) => mod.id !== featuredId);

  const stats = useMemo<BrowseStats>(
    () => ({
      // Browsing never scans the library, so unknown stays unknown.
      installed: modsResult ? modsResult.mods.length : null,
      updates: updatesResult
        ? updatesResult.updates.filter((update) => update.hasUpdate).length
        : null,
      // One download slot plus one archive batch, whichever is running.
      queue: downloading || installingArchive ? 1 : 0,
    }),
    [modsResult, updatesResult, downloading, installingArchive],
  );

  const focusCatalog = () => {
    catalogRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
    searchRef.current?.focus({ preventScroll: true });
  };

  // A "See all" click removes the storefront above the catalog, so scrolling in
  // the click handler lands mid-grid; wait for the filtered layout instead.
  useEffect(() => {
    if (!pendingCatalogFocus.current || !activeFilters) return;
    pendingCatalogFocus.current = false;
    focusCatalog();
  }, [activeFilters]);

  // The spotlight wants the full record for its gallery image and teaser.
  useEffect(() => {
    if (featuredId !== null) void loadSpotlightDetail(featuredId);
  }, [featuredId, loadSpotlightDetail]);

  return (
    <div className="mx-auto w-full max-w-[1500px] px-4 py-6 sm:px-8 sm:py-8">
      <header className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="font-display text-[26px] font-semibold tracking-tight text-zinc-50">
            Dashboard
          </h1>
          <p className="mt-1.5 max-w-xl text-[13px] leading-relaxed text-zinc-500">
            Discover featured, trending, and new mods.
          </p>
        </div>

        <Button variant="outline" size="sm" onClick={refresh} loading={isLoading}>
          <RefreshCw className="h-3.5 w-3.5" aria-hidden />
          Refresh
        </Button>
      </header>

      {unfiltered && !spotlight && !railsError && (isLoading || railsLoading) ? (
        <BrowseHeroSkeleton />
      ) : null}

      {spotlight ? (
        <BrowseHero
          mod={spotlight}
          detail={featuredDetail}
          stats={stats}
          onOpen={() => navigate(`/browse/${spotlight.id}`)}
          onBrowseStore={focusCatalog}
        />
      ) : null}

      {unfiltered
        ? shelves.map((shelf) => (
            <BrowseRail
              key={shelf.categoryId ?? "trending-now"}
              label={shelf.label}
              items={shelf.items}
              onOpen={(modId) => navigate(`/browse/${modId}`)}
              onSeeAll={
                shelf.categoryId === null
                  ? undefined
                  : () => {
                      pendingCatalogFocus.current = true;
                      setCategory(shelf.categoryId);
                    }
              }
            />
          ))
        : null}

      {unfiltered && railsLoading && shelves.length === 0 ? (
        <BrowseRailSkeleton />
      ) : null}

      {unfiltered && railsError ? (
        <div className="mt-4 flex flex-wrap items-center gap-3">
          <p className="font-mono text-[10px] text-zinc-600">
            Trending is unavailable right now: {railsError}
          </p>
          <Button size="sm" variant="ghost" onClick={() => void loadRails(true)}>
            Retry
          </Button>
        </div>
      ) : null}

      <div ref={catalogRef} id="browse-catalog" className="mt-10 scroll-mt-6">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
            <h2 className="font-display text-[15px] uppercase tracking-[0.16em] text-zinc-300">
              All mods
            </h2>
            {total > 0 ? (
              <span className="font-mono text-[9px] uppercase tracking-[0.16em] text-zinc-600">
                {total.toLocaleString()} on GameBanana
              </span>
            ) : null}
            {lastCached ? (
              <span className="rounded-full border border-ink-700 bg-ink-900 px-2 py-0.5 font-mono text-[8.5px] uppercase tracking-[0.16em] text-zinc-500">
                cached
              </span>
            ) : null}
          </div>

          <div
            role="group"
            aria-label="Result layout"
            className="flex items-center gap-0.5 rounded-lg border border-ink-700 bg-ink-900/70 p-0.5"
          >
            {(
              [
                { value: "grid", label: "Grid view", Icon: LayoutGrid },
                { value: "list", label: "List view", Icon: List },
              ] as const
            ).map(({ value, label, Icon }) => (
              <button
                key={value}
                type="button"
                onClick={() => setView(value)}
                aria-pressed={view === value}
                aria-label={label}
                title={label}
                className={cn(
                  "inline-flex h-8 w-8 items-center justify-center rounded-md transition-colors",
                  view === value
                    ? "bg-zinc-100 text-ink-950"
                    : "text-zinc-500 hover:text-zinc-200",
                )}
              >
                <Icon className="h-3.5 w-3.5" aria-hidden />
              </button>
            ))}
          </div>
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-2">
        <div className="relative min-w-[150px] flex-1">
          <Search
            className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-600"
            aria-hidden
          />
          <input
            ref={searchRef}
            value={searchInput}
            onChange={(event) => setSearchInput(event.target.value)}
            placeholder="Search mods, authors, tags..."
            aria-label="Search GameBanana"
            className="h-10 w-full border border-ink-600 bg-ink-900/70 pl-9 pr-9 text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-acid-400/60 focus:outline-none"
          />
          {searchInput ? (
            <button
              type="button"
              onClick={() => setSearchInput("")}
              aria-label="Clear search"
              className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-zinc-600 transition-colors hover:text-zinc-300"
            >
              <X className="h-3.5 w-3.5" aria-hidden />
            </button>
          ) : null}
        </div>

        <div className="relative">
          <select
            value={filters.sort}
            onChange={(event) => setSort(event.target.value as BrowseSort)}
            aria-label="Sort results"
            className="h-10 w-[164px] appearance-none border border-ink-600 bg-ink-900/70 pl-3 pr-8 font-display text-[10px] uppercase tracking-[0.16em] text-zinc-300 focus:border-acid-400/60 focus:outline-none"
          >
            {SORT_GROUPS.map((group) => (
              <optgroup key={group.label} label={group.label}>
                {group.options.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
          <ChevronDown
            className="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-500"
            aria-hidden
          />
        </div>

        <div className="relative">
          <select
            value={filters.categoryId ?? ""}
            onChange={(event) =>
              setCategory(event.target.value ? Number(event.target.value) : null)
            }
            aria-label="Filter by category"
            className="h-10 w-[164px] appearance-none border border-ink-600 bg-ink-900/70 pl-3 pr-8 font-display text-[10px] uppercase tracking-[0.16em] text-zinc-300 focus:border-acid-400/60 focus:outline-none"
          >
            <option value="">All categories</option>
            {categories.map((category) => (
              <option key={category.id} value={category.id}>
                {category.name}
              </option>
            ))}
          </select>
          <ChevronDown
            className="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-500"
            aria-hidden
          />
        </div>

        <SuggestionInput
          value={filters.character}
          onChange={setCharacter}
          suggestions={CHARACTER_SUGGESTIONS}
          placeholder="Character (e.g. Ellen)"
          ariaLabel="Filter by character"
          className="w-[164px]"
        />

        <div className="relative">
          <select
            value={filters.nsfw}
            onChange={(event) => setNsfw(event.target.value as NsfwFilter)}
            aria-label="Content filter"
            title="Filter by GameBanana's mature-content setting"
            className="h-10 w-[140px] appearance-none border border-ink-600 bg-ink-900/70 pl-3 pr-8 font-display text-[10px] uppercase tracking-[0.16em] text-zinc-300 focus:border-acid-400/60 focus:outline-none"
          >
            {NSFW_OPTIONS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
          <ChevronDown
            className="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-zinc-500"
            aria-hidden
          />
        </div>

        {activeFilters ? (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setSearchInput("");
              setSearch("");
              setCharacter("");
              setCategory(null);
            }}
          >
            Clear
          </Button>
        ) : null}
      </div>

      {filters.search.trim() ? (
        <p className="mt-2 font-mono text-[9px] text-zinc-600">
          Search results use GameBanana relevance order — sorting applies when
          browsing.
        </p>
      ) : null}

      {categoryError ? (
        <p className="mt-3 font-mono text-[10px] text-zinc-600">
          Categories unavailable: {categoryError}
        </p>
      ) : null}

      {error && dismissedError !== error ? (
        <Notice
          className="mt-6"
          title="GameBanana could not be reached"
          message={error}
          onDismiss={() => setDismissedError(error)}
          action={
            <Button size="sm" variant="outline" onClick={refresh}>
              Try again
            </Button>
          }
        />
      ) : null}

      {isLoading && items.length === 0 ? (
        <div className="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {Array.from({ length: 8 }).map((_, index) => (
            <BrowseCardSkeleton key={index} />
          ))}
        </div>
      ) : null}

      {!isLoading && !error && items.length === 0 ? (
        <Panel className="mt-6" eyebrow="No matches" title="Nothing found">
          <div className="flex flex-col items-center gap-3 py-8 text-center">
            <Compass className="h-8 w-8 text-zinc-700" aria-hidden />
            <p className="max-w-md text-xs leading-relaxed text-zinc-500">
              No mods matched this search and filter combination. Try a shorter
              search term, another character, widen the content filter, or
              clear the category filter.
            </p>
            <Button
              onClick={() => {
                setSearchInput("");
                setSearch("");
                setCharacter("");
                setCategory(null);
              }}
            >
              Clear filters
            </Button>
          </div>
        </Panel>
      ) : null}

      {isLoading && items.length > 0 ? (
        <p className="mt-3 flex items-center gap-2 font-mono text-[9px] uppercase tracking-[0.16em] text-zinc-500">
          <Loader2 className="h-3 w-3 animate-spin text-acid-400" aria-hidden />
          Loading results...
        </p>
      ) : null}

      {visibleItems.length > 0 ? (
        <div
          aria-busy={isLoading}
          className={cn(
            "mt-4 transition-opacity duration-200",
            isLoading && "opacity-60",
            view === "grid"
              ? "grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4"
              : "flex flex-col gap-2",
          )}
        >
          {visibleItems.map((mod) => (
            <BrowseCard
              key={mod.id}
              mod={mod}
              view={view}
              onOpen={() => navigate(`/browse/${mod.id}`)}
            />
          ))}
        </div>
      ) : null}

      <div ref={sentinelRef} aria-hidden className="h-10" />

      {isLoadingMore ? (
        <div className="flex items-center justify-center gap-2 py-4 font-display text-[10px] uppercase tracking-[0.2em] text-zinc-500">
          <Loader2 className="h-3.5 w-3.5 animate-spin text-acid-400" aria-hidden />
          Loading more mods…
        </div>
      ) : null}

      {!hasMore && items.length > 0 ? (
        <p className="py-4 text-center font-mono text-[10px] uppercase tracking-[0.2em] text-zinc-700">
          End of results
        </p>
      ) : null}

      {hasMore && !isLoadingMore && items.length > 0 ? (
        <div className="flex justify-center py-2">
          <Button variant="outline" size="sm" onClick={loadMore}>
            Load more
          </Button>
        </div>
      ) : null}
      </div>
    </div>
  );
}
