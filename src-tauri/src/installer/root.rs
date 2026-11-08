use std::fs;
use std::path::{Path, PathBuf};

/// Archives rarely nest mods deeper than this; the limit keeps pathological
/// trees from turning installation into a full-drive walk.
const MAX_SEARCH_DEPTH: usize = 6;

const JUNK_DIRS: &[&str] = &[".git", ".svn", "__MACOSX", "$RECYCLE.BIN"];
const JUNK_FILES: &[&str] = &[".ds_store", "thumbs.db", "desktop.ini"];

/// Find mod roots inside an extracted archive.
///
/// A folder that directly contains at least one `.ini` is a mod root. Nested
/// folders beneath a root are treated as part of that mod, while wrapper
/// folders without an INI are descended into — which is what makes archives
/// containing several mods work.
pub fn find_mod_roots(content: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    visit(content, 0, &mut roots);
    roots.sort();
    roots
}

fn visit(dir: &Path, depth: usize, roots: &mut Vec<PathBuf>) {
    if depth > MAX_SEARCH_DEPTH {
        return;
    }

    if has_ini_file(dir) {
        roots.push(dir.to_path_buf());
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut subdirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false)
        })
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            !name.starts_with('.') && !JUNK_DIRS.iter().any(|junk| name.eq_ignore_ascii_case(junk))
        })
        .collect();

    subdirs.sort();
    for subdir in subdirs {
        visit(&subdir, depth + 1, roots);
    }
}

pub fn has_ini_file(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };

    entries.flatten().any(|entry| {
        let is_file = entry
            .file_type()
            .map(|file_type| file_type.is_file())
            .unwrap_or(false);
        let is_ini = entry
            .path()
            .extension()
            .map(|extension| extension.to_string_lossy().eq_ignore_ascii_case("ini"))
            .unwrap_or(false);

        is_file && is_ini
    })
}

/// Remove packaging noise before installing so it does not end up inside the mod.
pub fn clean_junk(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            if name.eq_ignore_ascii_case("__MACOSX") {
                let _ = fs::remove_dir_all(&path);
            } else {
                clean_junk(&path);
            }
        } else if file_type.is_file()
            && JUNK_FILES
                .iter()
                .any(|junk| name.eq_ignore_ascii_case(junk))
        {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("zzmm-root-test-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, contents).expect("write file");
    }

    #[test]
    fn finds_multiple_mods_under_a_wrapper() {
        let root = temp_dir();
        write_file(&root.join("Pack/ModA/mod.ini"), "");
        write_file(&root.join("Pack/ModA/textures/extra.ini"), "");
        write_file(&root.join("Pack/ModB/mod.ini"), "");
        write_file(&root.join("Pack/readme.txt"), "");

        let roots = find_mod_roots(&root);
        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|path| path.ends_with("ModA")));
        assert!(roots.iter().any(|path| path.ends_with("ModB")));

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn archive_root_can_be_the_mod_root() {
        let root = temp_dir();
        write_file(&root.join("mod.ini"), "");
        write_file(&root.join("textures/extra.ini"), "");

        let roots = find_mod_roots(&root);
        assert_eq!(roots, vec![root.clone()]);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ignores_junk_and_empty_archives() {
        let root = temp_dir();
        fs::create_dir_all(root.join("__MACOSX")).expect("junk dir");
        write_file(&root.join("readme.txt"), "");

        assert!(find_mod_roots(&root).is_empty());

        clean_junk(&root);
        assert!(!root.join("__MACOSX").exists());

        fs::remove_dir_all(&root).ok();
    }
}
