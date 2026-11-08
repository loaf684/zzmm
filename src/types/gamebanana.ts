/** Types mirroring `src-tauri/src/api/model.rs`. */

/** Server-side orders. `likes`/`views`/`downloads` map to GameBanana's own
 *  `Generic_Most*` index sorts; search results always use relevance order. */
export type BrowseSort =
  | "newest"
  | "trending"
  | "updated"
  | "oldest"
  | "likes"
  | "views"
  | "downloads"
  | "name"
  | "name-desc";

export interface BrowseParams {
  cursor?: number | null;
  search?: string | null;
  categoryId?: number | null;
  categoryName?: string | null;
  /** Matched against GameBanana's subcategory name (e.g. "Ellen Joe"). */
  character?: string | null;
  sort?: BrowseSort;
}

export interface ModSummary {
  id: number;
  name: string;
  author: string | null;
  category: string | null;
  subCategory: string | null;
  likes: number;
  views: number;
  version: string | null;
  /** Unix seconds. */
  dateAdded: number;
  dateUpdated: number;
  thumbnailUrl: string | null;
  profileUrl: string;
  hasFiles: boolean;
}

export interface BrowsePage {
  items: ModSummary[];
  /** Opaque cursor for the next request; null when the list is exhausted. */
  nextCursor: number | null;
  total: number;
  hasMore: boolean;
  cached: boolean;
  source: string;
}

export interface ModFile {
  id: number;
  name: string;
  size: number;
  date: number;
  version: string | null;
  description: string | null;
  downloadUrl: string;
}

export interface ModDetail {
  id: number;
  name: string;
  author: string | null;
  /** GameBanana returns this as plain text, not HTML. */
  description: string;
  category: string | null;
  subCategory: string | null;
  version: string | null;
  likes: number;
  views: number;
  downloads: number;
  dateAdded: number;
  dateUpdated: number;
  profileUrl: string;
  thumbnailUrl: string | null;
  gallery: string[];
  files: ModFile[];
  obsolete: boolean;
  cached: boolean;
}

export interface CategoryChip {
  id: number;
  name: string;
  count: number;
}

export interface UpdateInfo {
  folderName: string;
  modId: number;
  name: string;
  installedFileId: number | null;
  latestFileId: number | null;
  installedVersion: string | null;
  latestVersion: string | null;
  latestFileDate: number | null;
  thumbnailUrl: string | null;
  hasUpdate: boolean;
  error: string | null;
}

export interface UpdateCheckResult {
  checked: number;
  updates: UpdateInfo[];
}

export type DownloadStage = "downloading" | "installing" | "done" | "failed";

export interface DownloadProgress {
  modId: number;
  name: string;
  stage: DownloadStage;
  percent: number;
  received: number;
  total: number;
  message: string;
}

export interface DownloadOutcome {
  modId: number;
  modName: string;
  fileId: number;
  fileName: string;
  installed: string[];
  warnings: string[];
  replacedFolder: string | null;
}
