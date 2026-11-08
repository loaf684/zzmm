use std::path::Path;
use std::time::Duration;

use crate::api::cache;
use crate::api::client::{encode, get_json};
use crate::api::model::{
    to_detail, BrowsePage, BrowseParams, CategoryChip, ModDetail, RawDetail, RawGameProfile,
    RawPage, RawRecord, UpdateCheckResult, UpdateInfo,
};
use crate::api::GAME_ID;
use crate::error::{AppError, AppResult};
use crate::mods::{metadata, scanner};

const BASE: &str = "https://gamebanana.com/apiv11";

/// Mods per request. `Mod/Index` accepts `_nPerpage`, the other endpoints use a
/// fixed 15, so this is only a request hint.
const PAGE_SIZE: u32 = 30;

/// How many server pages a single request may walk when a filter can only be
/// applied client-side (character / category name). Keeps a filtered page from
/// turning into an API crawl.
const LOOKAHEAD_PAGES: u32 = 8;

const BROWSE_TTL: Duration = Duration::from_secs(30 * 60);
const DETAIL_TTL: Duration = Duration::from_secs(2 * 60 * 60);
const CATEGORY_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const UPDATE_TTL: Duration = Duration::from_secs(30 * 60);

/// Cap on update checks per run so a large library cannot hammer the API.
const MAX_UPDATE_CHECKS: usize = 25;

/// Sorting options GameBanana exposes.
///
/// `Mod/Index` accepts the `Generic_*` aliases advertised by
/// `/Mod/ListFilterConfig` (most liked / viewed / downloaded, oldest, A-Z), so
/// those sort server-side across the whole catalogue. `trending` and `updated`
/// come from the game subfeed, whose `_sSort` only knows `default`/`updated`;
/// `default` is GameBanana's engagement-ordered feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Newest,
    Trending,
    Updated,
    Oldest,
    MostLiked,
    MostViewed,
    MostDownloaded,
    NameAsc,
    NameDesc,
}

impl Sort {
    fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("newest").to_ascii_lowercase().as_str() {
            "trending" | "popular" => Sort::Trending,
            "updated" => Sort::Updated,
            "oldest" => Sort::Oldest,
            "likes" | "most-liked" => Sort::MostLiked,
            "views" | "most-viewed" => Sort::MostViewed,
            "downloads" | "most-downloaded" => Sort::MostDownloaded,
            "name" | "a-z" => Sort::NameAsc,
            "name-desc" | "z-a" => Sort::NameDesc,
            _ => Sort::Newest,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Sort::Newest => "newest",
            Sort::Trending => "trending",
            Sort::Updated => "updated",
            Sort::Oldest => "oldest",
            Sort::MostLiked => "likes",
            Sort::MostViewed => "views",
            Sort::MostDownloaded => "downloads",
            Sort::NameAsc => "name",
            Sort::NameDesc => "name-desc",
        }
    }

    fn uses_subfeed(self) -> bool {
        matches!(self, Sort::Trending | Sort::Updated)
    }

    /// `_sSort` alias for `Mod/Index`; `None` keeps the endpoint's own order.
    fn index_alias(self) -> Option<&'static str> {
        match self {
            Sort::Oldest => Some("Generic_Oldest"),
            Sort::MostLiked => Some("Generic_MostLiked"),
            Sort::MostViewed => Some("Generic_MostViewed"),
            Sort::MostDownloaded => Some("Generic_MostDownloaded"),
            Sort::NameAsc => Some("Generic_Alphabetically"),
            Sort::NameDesc => Some("Generic_ReverseAlphabetically"),
            _ => None,
        }
    }
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// URLs
// ---------------------------------------------------------------------------

fn index_url(category_id: Option<u64>, page: u32, sort: Sort) -> String {
    // Bracket filters are percent-encoded because reqwest does not pre-encode
    // the query string for us.
    let filter = match category_id {
        Some(id) => format!("_aFilters%5BGeneric_Category%5D={id}"),
        None => format!("_aFilters%5BGeneric_Game%5D={GAME_ID}"),
    };
    let order = match sort.index_alias() {
        Some(alias) => format!("&_sSort={alias}"),
        None => String::new(),
    };

    format!("{BASE}/Mod/Index?_nPerpage={PAGE_SIZE}&_nPage={page}{order}&{filter}")
}

fn subfeed_url(sort: Sort, page: u32) -> String {
    let order = match sort {
        Sort::Updated => "updated",
        _ => "default",
    };

    format!("{BASE}/Game/{GAME_ID}/Subfeed?_nPage={page}&_sSort={order}&_csvModelInclusions=Mod")
}

fn search_url(query: &str, page: u32) -> String {
    format!(
        "{BASE}/Util/Search/Results?_sSearchString={}&_idGameRow={GAME_ID}&_sModelName=Mod&_nPage={page}",
        encode(query)
    )
}

// ---------------------------------------------------------------------------
// Browsing
// ---------------------------------------------------------------------------

pub async fn browse(cache_dir: &Path, params: &BrowseParams) -> AppResult<BrowsePage> {
    let cursor = params.cursor.unwrap_or(1).max(1);
    let search = trimmed(params.search.as_deref());
    let character = trimmed(params.character.as_deref());
    let category_name = trimmed(params.category_name.as_deref());
    let sort = Sort::parse(params.sort.as_deref());

    let key = format!(
        "browse|{cursor}|{}|{}|{}|{}|{}",
        sort.key(),
        search.unwrap_or_default().to_lowercase(),
        params
            .category_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        category_name.unwrap_or_default().to_lowercase(),
        character.unwrap_or_default().to_lowercase()
    );

    if let Some(mut page) = cache::read::<BrowsePage>(cache_dir, &key, BROWSE_TTL) {
        page.cached = true;
        return Ok(page);
    }

    // Category can only be filtered server-side on `Mod/Index`. Search results
    // and the sorted subfeed need a client-side category match, which means
    // walking a few extra pages to fill one screen.
    let check_category = sort.uses_subfeed() || search.is_some();
    let filtering = character.is_some() || (check_category && category_name.is_some());
    let max_pages = if filtering { LOOKAHEAD_PAGES } else { 1 };

    let mut server_page = cursor;
    let mut items = Vec::new();
    let mut total = 0u32;
    let mut has_more = true;
    let mut source = "index";

    for _ in 0..max_pages {
        let raw = if let Some(query) = search {
            source = "search";
            get_json::<RawPage>(&search_url(query, server_page)).await?
        } else if sort.uses_subfeed() {
            source = "subfeed";
            get_json::<RawPage>(&subfeed_url(sort, server_page)).await?
        } else {
            source = "index";
            get_json::<RawPage>(&index_url(params.category_id, server_page, sort)).await?
        };

        total = raw.metadata.record_count.unwrap_or(total);

        if raw.records.is_empty() {
            has_more = false;
            break;
        }

        for record in &raw.records {
            if matches_filters(record, category_name, character, check_category) {
                items.push(crate::api::model::summarize(record));
            }
        }

        if raw.metadata.is_complete.unwrap_or(false) {
            has_more = false;
        }

        server_page += 1;

        if !has_more || items.len() >= PAGE_SIZE as usize {
            break;
        }
    }

    let page = BrowsePage {
        items,
        next_cursor: if has_more { Some(server_page) } else { None },
        total,
        has_more,
        cached: false,
        source: source.to_string(),
    };

    cache::write(cache_dir, &key, &page);
    Ok(page)
}

fn matches_filters(
    record: &RawRecord,
    category_name: Option<&str>,
    character: Option<&str>,
    check_category: bool,
) -> bool {
    if let Some(character) = character {
        let wanted = character.to_lowercase();
        let sub_category = record
            .sub_category
            .as_ref()
            .and_then(|category| category.name.as_deref())
            .unwrap_or_default()
            .to_lowercase();

        // Mods in root categories (UI packs, sounds, ...) have no character.
        if !sub_category.contains(&wanted) {
            return false;
        }
    }

    if check_category {
        if let Some(category) = category_name {
            let root = record
                .root_category
                .as_ref()
                .and_then(|value| value.name.as_deref())
                .unwrap_or_default();

            if !root.eq_ignore_ascii_case(category) {
                return false;
            }
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Detail
// ---------------------------------------------------------------------------

pub async fn detail(cache_dir: &Path, mod_id: u64, ttl: Duration) -> AppResult<ModDetail> {
    let key = format!("detail|{mod_id}");

    if let Some(mut detail) = cache::read::<ModDetail>(cache_dir, &key, ttl) {
        detail.cached = true;
        return Ok(detail);
    }

    let raw: RawDetail = get_json(&format!("{BASE}/Mod/{mod_id}/ProfilePage")).await?;
    if raw.id == 0 {
        return Err(AppError::GameBanana(format!(
            "GameBanana did not return a mod for ID {mod_id}."
        )));
    }

    let detail = to_detail(raw);
    cache::write(cache_dir, &key, &detail);
    Ok(detail)
}

pub async fn detail_for_browse(cache_dir: &Path, mod_id: u64) -> AppResult<ModDetail> {
    detail(cache_dir, mod_id, DETAIL_TTL).await
}

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

/// Root mod categories (Character Skins, UI, ...). GameBanana has no working
/// apiv11 endpoint for the subcategory tree, so characters are matched by name
/// against each record's subcategory instead.
pub async fn categories(cache_dir: &Path) -> AppResult<Vec<CategoryChip>> {
    const KEY: &str = "categories|game-roots";

    if let Some(chips) = cache::read::<Vec<CategoryChip>>(cache_dir, KEY, CATEGORY_TTL) {
        return Ok(chips);
    }

    let raw: RawGameProfile = get_json(&format!("{BASE}/Game/{GAME_ID}/ProfilePage")).await?;

    let chips: Vec<CategoryChip> = raw
        .root_categories
        .into_iter()
        .filter_map(|category| {
            let id = category.id?;
            let name = category.name?.trim().to_string();
            if id == 0 || name.is_empty() {
                return None;
            }
            Some(CategoryChip {
                id,
                name,
                count: category.item_count.unwrap_or(0),
            })
        })
        .collect();

    cache::write(cache_dir, KEY, &chips);
    Ok(chips)
}

// ---------------------------------------------------------------------------
// Update checks
// ---------------------------------------------------------------------------

pub async fn updates(cache_dir: &Path, mods_dir: &Path) -> AppResult<UpdateCheckResult> {
    let linked = collect_linked_mods(mods_dir);
    let mut updates = Vec::with_capacity(linked.len());

    for (folder_name, meta) in linked.into_iter() {
        // The HTTP client spaces and serializes actual requests. Cached checks
        // can therefore run immediately without adding a fixed delay each.
        let Some(mod_id) = meta.game_banana_mod_id else {
            continue;
        };

        let base = UpdateInfo {
            folder_name: folder_name.clone(),
            mod_id,
            name: meta.name.clone().unwrap_or_else(|| {
                folder_name
                    .trim_start_matches(scanner::DISABLED_PREFIX)
                    .to_string()
            }),
            installed_file_id: meta.game_banana_file_id,
            latest_file_id: None,
            installed_version: meta.version.clone(),
            latest_version: None,
            latest_file_date: None,
            thumbnail_url: meta.thumbnail_url.clone(),
            has_update: false,
            error: None,
        };

        match detail(cache_dir, mod_id, UPDATE_TTL).await {
            Ok(detail) => {
                let latest = detail.files.first();
                let latest_file_id = latest.map(|file| file.id);
                let latest_version = detail
                    .version
                    .clone()
                    .or_else(|| latest.and_then(|file| file.version.clone()));

                updates.push(UpdateInfo {
                    latest_file_id,
                    latest_version: latest_version.clone(),
                    latest_file_date: detail.files.first().map(|file| file.date),
                    has_update: has_update(&meta, latest_file_id, latest_version.as_deref()),
                    ..base
                });
            }
            Err(error) => {
                log::warn!("Update check failed for {folder_name}: {error}");
                updates.push(UpdateInfo {
                    error: Some(error.to_string()),
                    ..base
                });
            }
        }
    }

    // Newest-available first, then by name, so the banner is stable.
    updates.sort_by(|a, b| {
        b.has_update
            .cmp(&a.has_update)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(UpdateCheckResult {
        checked: updates.len() as u32,
        updates,
    })
}

fn has_update(
    installed: &metadata::ModMetadata,
    latest_file_id: Option<u64>,
    latest_version: Option<&str>,
) -> bool {
    if let (Some(installed_id), Some(latest_id)) = (installed.game_banana_file_id, latest_file_id) {
        return installed_id != latest_id;
    }

    // Older metadata may only carry a version string.
    match (installed.version.as_deref(), latest_version) {
        (Some(installed), Some(latest)) => {
            !installed.trim().is_empty() && !installed.trim().eq_ignore_ascii_case(latest.trim())
        }
        _ => false,
    }
}

/// Read sidecar metadata for every mod folder. Purely local and bounded, so it
/// runs on the calling (blocking) thread instead of a full hash scan.
fn collect_linked_mods(mods_dir: &Path) -> Vec<(String, metadata::ModMetadata)> {
    let mut found = Vec::new();

    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return found;
    };

    for entry in entries.flatten() {
        let folder_name = entry.file_name().to_string_lossy().to_string();
        if folder_name.starts_with('.') {
            continue;
        }
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }

        if let Some(meta) = metadata::read(&entry.path()) {
            if meta.game_banana_mod_id.is_some() {
                found.push((folder_name, meta));
            }
        }

        if found.len() >= MAX_UPDATE_CHECKS {
            break;
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::model::RawCategory;

    fn record(root: &str, sub: &str) -> RawRecord {
        RawRecord {
            root_category: Some(RawCategory {
                name: Some(root.to_string()),
            }),
            sub_category: Some(RawCategory {
                name: Some(sub.to_string()),
            }),
            ..RawRecord::default()
        }
    }

    #[test]
    fn filters_by_character_and_category() {
        let ellen = record("Character Skins", "Ellen Joe");

        assert!(matches_filters(&ellen, None, Some("ellen"), false));
        assert!(matches_filters(&ellen, Some("character skins"), None, true));
        assert!(!matches_filters(&ellen, Some("UI"), None, true));
        assert!(!matches_filters(&ellen, None, Some("nicole"), false));
        assert!(matches_filters(&ellen, None, None, false));
    }

    #[test]
    fn update_detection_compares_file_ids_then_versions() {
        let mut meta = metadata::ModMetadata {
            game_banana_mod_id: Some(1),
            game_banana_file_id: Some(10),
            ..metadata::ModMetadata::default()
        };

        assert!(has_update(&meta, Some(11), None));
        assert!(!has_update(&meta, Some(10), None));

        meta.game_banana_file_id = None;
        meta.version = Some("1.0".into());
        assert!(has_update(&meta, Some(11), Some("1.1")));
        assert!(!has_update(&meta, Some(11), Some("1.0")));
    }

    #[test]
    fn builds_expected_urls() {
        assert!(index_url(None, 1, Sort::Newest).contains("Generic_Game%5D=19567"));
        assert!(index_url(Some(30305), 2, Sort::Newest).contains("Generic_Category%5D=30305"));
        assert!(!index_url(None, 1, Sort::Newest).contains("_sSort"));
        assert!(subfeed_url(Sort::Updated, 3).contains("_sSort=updated"));
        assert!(search_url("ellen joe", 1).contains("_sSearchString=ellen%20joe"));
        assert!(index_url(None, 1, Sort::MostLiked).contains("_sSort=Generic_MostLiked"));
        assert!(index_url(None, 1, Sort::MostDownloaded).contains("_sSort=Generic_MostDownloaded"));
        assert_eq!(Sort::parse(Some("views")), Sort::MostViewed);
        assert!(!Sort::MostViewed.uses_subfeed());
    }
}
