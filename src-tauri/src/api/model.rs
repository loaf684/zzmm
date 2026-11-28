use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Wire models.
//
// GameBanana's apiv11 payloads change over time and omit fields per model, so
// every value here is optional and unknown keys are ignored. Only the fields
// the UI actually needs are mapped.
// ---------------------------------------------------------------------------

/// How GameBanana gates a submission behind its mature-content warning.
///
/// The apiv11 mod payloads carry no `_bIsNsfw`; the signal is
/// `_sInitialVisibility` (`show` / `warn` / `hide`), corroborated by
/// `_bHasContentRatings` on list endpoints and `_aContentRatings` on the
/// profile page. `show` with no ratings is the only thing this app treats as
/// safe: a payload with neither field is [`NsfwLevel::Unknown`], never safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NsfwLevel {
    Safe,
    Nsfw,
    Unknown,
}

/// Classify a submission from its visibility string and whether it carries
/// content ratings. `visibility` is trimmed and matched case-insensitively so a
/// stray casing or padding cannot silently downgrade an explicit submission.
pub fn nsfw_level(visibility: Option<&str>, rated: bool) -> NsfwLevel {
    let visibility = visibility.map(str::trim).filter(|value| !value.is_empty());

    match visibility.map(str::to_ascii_lowercase).as_deref() {
        // Rated content is adult even when the submitter left it visible.
        Some("show") => {
            if rated {
                NsfwLevel::Nsfw
            } else {
                NsfwLevel::Safe
            }
        }
        Some("warn") | Some("hide") => NsfwLevel::Nsfw,
        // An unrecognised visibility value is no evidence of safety.
        _ => {
            if rated {
                NsfwLevel::Nsfw
            } else {
                NsfwLevel::Unknown
            }
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawPage {
    #[serde(rename = "_aMetadata", default)]
    pub metadata: RawPageMetadata,
    #[serde(rename = "_aRecords", default)]
    pub records: Vec<RawRecord>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawPageMetadata {
    #[serde(rename = "_nRecordCount", default)]
    pub record_count: Option<u32>,
    #[serde(rename = "_bIsComplete", default)]
    pub is_complete: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawRecord {
    #[serde(rename = "_idRow", default)]
    pub id: u64,
    #[serde(rename = "_sName", default)]
    pub name: String,
    #[serde(rename = "_sProfileUrl", default)]
    pub profile_url: Option<String>,
    #[serde(rename = "_tsDateAdded", default)]
    pub date_added: Option<i64>,
    #[serde(rename = "_tsDateModified", default)]
    pub date_modified: Option<i64>,
    #[serde(rename = "_tsDateUpdated", default)]
    pub date_updated: Option<i64>,
    #[serde(rename = "_sVersion", default)]
    pub version: Option<String>,
    #[serde(rename = "_nLikeCount", default)]
    pub like_count: Option<u32>,
    #[serde(rename = "_nViewCount", default)]
    pub view_count: Option<u32>,
    #[serde(rename = "_bHasFiles", default)]
    pub has_files: Option<bool>,
    /// `show` / `warn` / `hide`: GameBanana's mature-content gate.
    #[serde(rename = "_sInitialVisibility", default)]
    pub initial_visibility: Option<String>,
    /// Content ratings exist on this submission (the codes themselves are only
    /// on the profile page).
    #[serde(rename = "_bHasContentRatings", default)]
    pub has_content_ratings: Option<bool>,
    #[serde(rename = "_aPreviewMedia", default)]
    pub preview_media: Option<RawPreviewMedia>,
    #[serde(rename = "_aSubmitter", default)]
    pub submitter: Option<RawSubmitter>,
    #[serde(rename = "_aRootCategory", default)]
    pub root_category: Option<RawCategory>,
    #[serde(rename = "_aSubCategory", default)]
    pub sub_category: Option<RawCategory>,
}

impl RawRecord {
    pub fn nsfw_level(&self) -> NsfwLevel {
        nsfw_level(
            self.initial_visibility.as_deref(),
            self.has_content_ratings.unwrap_or(false),
        )
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawPreviewMedia {
    #[serde(rename = "_aImages", default)]
    pub images: Vec<RawImage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawImage {
    #[serde(rename = "_sBaseUrl", default)]
    pub base_url: Option<String>,
    #[serde(rename = "_sFile", default)]
    pub file: Option<String>,
    #[serde(rename = "_sFile220", default)]
    pub file_220: Option<String>,
    #[serde(rename = "_sFile530", default)]
    pub file_530: Option<String>,
    #[serde(rename = "_sFile800", default)]
    pub file_800: Option<String>,
    /// Game icons and banners use a fully-qualified URL instead of base + file.
    #[serde(rename = "_sUrl", default)]
    pub url: Option<String>,
}

impl RawImage {
    fn join(base: Option<&str>, file: Option<&str>) -> Option<String> {
        match (base, file) {
            (Some(base), Some(file)) if !file.is_empty() => {
                Some(format!("{}/{}", base.trim_end_matches('/'), file))
            }
            _ => None,
        }
    }

    /// Original image, suitable for the detail gallery.
    pub fn full(&self) -> Option<String> {
        self.url
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| Self::join(self.base_url.as_deref(), self.file.as_deref()))
    }

    /// ~530px variant: big enough for a card, small enough to load quickly.
    pub fn medium(&self) -> Option<String> {
        self.url
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| Self::join(self.base_url.as_deref(), self.file_530.as_deref()))
            .or_else(|| self.full())
    }

    pub fn thumbnail(&self) -> Option<String> {
        self.url
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| Self::join(self.base_url.as_deref(), self.file_220.as_deref()))
            .or_else(|| self.medium())
    }

    pub fn gallery(&self) -> Option<String> {
        self.url
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| Self::join(self.base_url.as_deref(), self.file_800.as_deref()))
            .or_else(|| self.full())
    }
}

impl RawPreviewMedia {
    pub fn thumbnail(&self) -> Option<String> {
        self.images.first().and_then(RawImage::thumbnail)
    }

    pub fn gallery(&self) -> Vec<String> {
        self.images.iter().filter_map(RawImage::gallery).collect()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawSubmitter {
    #[serde(rename = "_sName", default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawCategory {
    #[serde(rename = "_sName", default)]
    pub name: Option<String>,
}

impl RawCategory {
    fn label(&self) -> Option<String> {
        self.name
            .clone()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawFile {
    #[serde(rename = "_idRow", default)]
    pub id: u64,
    #[serde(rename = "_sFile", default)]
    pub file_name: Option<String>,
    #[serde(rename = "_nFilesize", default)]
    pub size: Option<u64>,
    #[serde(rename = "_tsDateAdded", default)]
    pub date_added: Option<i64>,
    #[serde(rename = "_sDownloadUrl", default)]
    pub download_url: Option<String>,
    #[serde(rename = "_sVersion", default)]
    pub version: Option<String>,
    #[serde(rename = "_sDescription", default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawDetail {
    #[serde(rename = "_idRow", default)]
    pub id: u64,
    #[serde(rename = "_sName", default)]
    pub name: String,
    #[serde(rename = "_sProfileUrl", default)]
    pub profile_url: Option<String>,
    /// GameBanana returns the description as plain text (not HTML).
    #[serde(rename = "_sText", default)]
    pub text: Option<String>,
    #[serde(rename = "_sVersion", default)]
    pub version: Option<String>,
    #[serde(rename = "_nLikeCount", default)]
    pub like_count: Option<u32>,
    #[serde(rename = "_nViewCount", default)]
    pub view_count: Option<u32>,
    #[serde(rename = "_nDownloadCount", default)]
    pub download_count: Option<u32>,
    #[serde(rename = "_tsDateAdded", default)]
    pub date_added: Option<i64>,
    #[serde(rename = "_tsDateModified", default)]
    pub date_modified: Option<i64>,
    #[serde(rename = "_aFiles", default)]
    pub files: Vec<RawFile>,
    #[serde(rename = "_aPreviewMedia", default)]
    pub preview_media: Option<RawPreviewMedia>,
    #[serde(rename = "_aSubmitter", default)]
    pub submitter: Option<RawSubmitter>,
    #[serde(rename = "_aCategory", default)]
    pub category: Option<RawCategory>,
    /// Root category ("Character Skins", "UI", "Bangboo Skins", ...). The
    /// profile endpoint names it here instead of `_aRootCategory`.
    #[serde(rename = "_aSuperCategory", default)]
    pub super_category: Option<RawCategory>,
    #[serde(rename = "_aRootCategory", default)]
    pub root_category: Option<RawCategory>,
    #[serde(rename = "_aSubCategory", default)]
    pub sub_category: Option<RawCategory>,
    #[serde(rename = "_bIsObsolete", default)]
    pub is_obsolete: Option<bool>,
    /// `show` / `warn` / `hide`: GameBanana's mature-content gate.
    #[serde(rename = "_sInitialVisibility", default)]
    pub initial_visibility: Option<String>,
    /// Rating codes to titles (`{"sa": "Skimpy Attire", ...}`); empty or absent
    /// on submissions GameBanana does not rate.
    #[serde(rename = "_aContentRatings", default)]
    pub content_ratings: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawGameProfile {
    #[serde(rename = "_aModRootCategories", default)]
    pub root_categories: Vec<RawRootCategory>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawRootCategory {
    #[serde(rename = "_idRow", default)]
    pub id: Option<u64>,
    #[serde(rename = "_sName", default)]
    pub name: Option<String>,
    #[serde(rename = "_nItemCount", default)]
    pub item_count: Option<u32>,
}

// ---------------------------------------------------------------------------
// View models sent to the webview.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowseParams {
    /// Server page to start from. `None` means the first page.
    pub cursor: Option<u32>,
    pub search: Option<String>,
    pub category_id: Option<u64>,
    /// Category name is needed for endpoints that cannot filter by ID
    /// server-side (search and the sorted subfeed).
    pub category_name: Option<String>,
    /// Matched against GameBanana's subcategory name (the character).
    pub character: Option<String>,
    /// `newest` | `trending` | `updated` | `oldest` | `likes` | `views` |
    /// `downloads` | `name` | `name-desc`.
    pub sort: Option<String>,
    /// `all` | `sfw` | `nsfw`; absent means `all`.
    pub nsfw: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSummary {
    pub id: u64,
    pub name: String,
    pub author: Option<String>,
    pub category: Option<String>,
    pub sub_category: Option<String>,
    pub likes: u32,
    pub views: u32,
    pub version: Option<String>,
    pub date_added: i64,
    pub date_updated: i64,
    pub thumbnail_url: Option<String>,
    pub profile_url: String,
    pub has_files: bool,
    /// Mature-content classification; `unknown` is never treated as safe.
    pub nsfw: NsfwLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePage {
    pub items: Vec<ModSummary>,
    pub next_cursor: Option<u32>,
    pub total: u32,
    pub has_more: bool,
    /// True when the page was served from the on-disk TTL cache.
    #[serde(default)]
    pub cached: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub date: i64,
    pub version: Option<String>,
    pub description: Option<String>,
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModDetail {
    pub id: u64,
    pub name: String,
    pub author: Option<String>,
    pub description: String,
    pub category: Option<String>,
    pub sub_category: Option<String>,
    /// GameBanana's root category, e.g. "Character Skins", "UI" or
    /// "Bangboo Skins". Used to classify non-character downloads.
    pub super_category: Option<String>,
    pub version: Option<String>,
    pub likes: u32,
    pub views: u32,
    pub downloads: u32,
    pub date_added: i64,
    pub date_updated: i64,
    pub profile_url: String,
    pub thumbnail_url: Option<String>,
    pub gallery: Vec<String>,
    pub files: Vec<ModFile>,
    pub obsolete: bool,
    #[serde(default)]
    pub cached: bool,
    /// Mature-content classification; `unknown` is never treated as safe.
    pub nsfw: NsfwLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryChip {
    pub id: u64,
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub folder_name: String,
    pub mod_id: u64,
    pub name: String,
    pub installed_file_id: Option<u64>,
    pub latest_file_id: Option<u64>,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub latest_file_date: Option<i64>,
    pub thumbnail_url: Option<String>,
    pub has_update: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub checked: u32,
    pub updates: Vec<UpdateInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub mod_id: u64,
    pub name: String,
    /// `downloading` | `installing` | `done` | `failed`
    pub stage: String,
    pub percent: u8,
    pub received: u64,
    pub total: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadOutcome {
    pub mod_id: u64,
    pub mod_name: String,
    pub file_id: u64,
    pub file_name: String,
    pub installed: Vec<String>,
    pub warnings: Vec<String>,
    /// Previously installed folder that was disabled because it was updated.
    pub replaced_folder: Option<String>,
}

// ---------------------------------------------------------------------------
// Mapping helpers
// ---------------------------------------------------------------------------

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn summarize(record: &RawRecord) -> ModSummary {
    let date_added = record.date_added.unwrap_or(0);

    ModSummary {
        id: record.id,
        name: record.name.clone(),
        author: record
            .submitter
            .as_ref()
            .and_then(|submitter| non_empty(submitter.name.clone())),
        category: record.root_category.as_ref().and_then(RawCategory::label),
        sub_category: record.sub_category.as_ref().and_then(RawCategory::label),
        likes: record.like_count.unwrap_or(0),
        views: record.view_count.unwrap_or(0),
        version: non_empty(record.version.clone()),
        date_added,
        date_updated: record
            .date_updated
            .or(record.date_modified)
            .unwrap_or(date_added),
        thumbnail_url: record
            .preview_media
            .as_ref()
            .and_then(RawPreviewMedia::thumbnail),
        profile_url: record
            .profile_url
            .clone()
            .unwrap_or_else(|| format!("https://gamebanana.com/mods/{}", record.id)),
        has_files: record.has_files.unwrap_or(false),
        nsfw: record.nsfw_level(),
    }
}

pub fn to_detail(raw: RawDetail) -> ModDetail {
    let mut files: Vec<ModFile> = raw
        .files
        .iter()
        .filter(|file| file.id != 0)
        .map(|file| ModFile {
            id: file.id,
            name: non_empty(file.file_name.clone()).unwrap_or_else(|| format!("file-{}", file.id)),
            size: file.size.unwrap_or(0),
            date: file.date_added.unwrap_or(0),
            version: non_empty(file.version.clone()),
            description: non_empty(file.description.clone()),
            download_url: file
                .download_url
                .clone()
                .filter(|url| !url.is_empty())
                .unwrap_or_else(|| format!("https://gamebanana.com/dl/{}", file.id)),
        })
        .collect();

    // Newest first: that is what "install" defaults to.
    files.sort_by(|a, b| b.date.cmp(&a.date));

    let preview = raw.preview_media.as_ref();
    let date_added = raw.date_added.unwrap_or(0);

    ModDetail {
        id: raw.id,
        name: raw.name,
        author: raw
            .submitter
            .as_ref()
            .and_then(|submitter| non_empty(submitter.name.clone())),
        description: raw
            .text
            .unwrap_or_default()
            .replace("\r\n", "\n")
            .trim()
            .to_string(),
        category: raw
            .category
            .as_ref()
            .and_then(RawCategory::label)
            .or_else(|| raw.root_category.as_ref().and_then(RawCategory::label)),
        sub_category: raw.sub_category.as_ref().and_then(RawCategory::label),
        super_category: raw.super_category.as_ref().and_then(RawCategory::label),
        version: non_empty(raw.version.clone()),
        likes: raw.like_count.unwrap_or(0),
        views: raw.view_count.unwrap_or(0),
        downloads: raw.download_count.unwrap_or(0),
        date_added,
        date_updated: raw.date_modified.unwrap_or(date_added),
        profile_url: raw
            .profile_url
            .clone()
            .unwrap_or_else(|| format!("https://gamebanana.com/mods/{}", raw.id)),
        thumbnail_url: preview.and_then(RawPreviewMedia::thumbnail),
        gallery: preview.map(RawPreviewMedia::gallery).unwrap_or_default(),
        files,
        obsolete: raw.is_obsolete.unwrap_or(false),
        cached: false,
        nsfw: nsfw_level(
            raw.initial_visibility.as_deref(),
            raw.content_ratings
                .as_ref()
                .map(|ratings| !ratings.is_empty())
                .unwrap_or(false),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_visibility_and_ratings() {
        // `show` without ratings is the only signal this app calls safe.
        assert_eq!(nsfw_level(Some("show"), false), NsfwLevel::Safe);
        assert_eq!(nsfw_level(Some(" SHOW "), false), NsfwLevel::Safe);
        assert_eq!(nsfw_level(Some("warn"), false), NsfwLevel::Nsfw);
        assert_eq!(nsfw_level(Some("hide"), false), NsfwLevel::Nsfw);
        // Ratings make a nominally visible submission adult.
        assert_eq!(nsfw_level(Some("show"), true), NsfwLevel::Nsfw);
        // "Missing" must never decode as safe.
        assert_eq!(nsfw_level(None, false), NsfwLevel::Unknown);
        assert_eq!(nsfw_level(Some(""), false), NsfwLevel::Unknown);
        assert_eq!(nsfw_level(Some("mystery"), false), NsfwLevel::Unknown);
        // Ratings alone are still adult.
        assert_eq!(nsfw_level(None, true), NsfwLevel::Nsfw);
    }

    #[test]
    fn record_level_reads_the_wire_fields() {
        let safe = RawRecord {
            initial_visibility: Some("show".into()),
            has_content_ratings: Some(false),
            ..RawRecord::default()
        };
        let adult = RawRecord {
            initial_visibility: Some("hide".into()),
            has_content_ratings: Some(true),
            ..RawRecord::default()
        };

        assert_eq!(safe.nsfw_level(), NsfwLevel::Safe);
        assert_eq!(adult.nsfw_level(), NsfwLevel::Nsfw);
        assert_eq!(RawRecord::default().nsfw_level(), NsfwLevel::Unknown);
    }
}
