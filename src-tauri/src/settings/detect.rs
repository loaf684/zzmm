use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

use crate::settings::model::DetectedModsDir;
use crate::settings::paths::inspect_mods_dir;

struct CandidateRoot {
    path: PathBuf,
    label: &'static str,
}

/// Look for ZZMI Mods folders in common XXMI Launcher / ZZMI locations.
///
/// This deliberately probes a fixed list instead of walking whole drives: a
/// recursive search would be slow and could touch folders the user did not
/// expect ZZMM to open.
pub fn find_installs() -> Vec<DetectedModsDir> {
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut found = Vec::new();

    for root in candidate_roots() {
        if !root.path.is_dir() {
            continue;
        }

        for mods_dir in mods_dirs_under(&root.path) {
            if !mods_dir.is_dir() {
                continue;
            }

            // Canonicalising resolves symlinks/junctions so the same folder is
            // not listed twice (e.g. env-var path vs. drive-level path).
            let dedupe_key = std::fs::canonicalize(&mods_dir)
                .unwrap_or_else(|_| mods_dir.clone())
                .to_string_lossy()
                .to_lowercase();
            if !seen_paths.insert(dedupe_key) {
                continue;
            }

            let state = inspect_mods_dir(&mods_dir);
            let relative = mods_dir
                .strip_prefix(&root.path)
                .map(|path| path.to_string_lossy().replace('\\', " / "))
                .unwrap_or_default();

            let label = if relative.is_empty() {
                root.label.to_string()
            } else {
                format!("{} · {}", root.label, relative)
            };

            found.push(DetectedModsDir {
                path: mods_dir.to_string_lossy().to_string(),
                label,
                source: source_for(&mods_dir).to_string(),
                mod_count: state.mod_count,
                disabled_count: state.disabled_count,
            });
        }
    }

    // Prefer launcher-managed installs, then richer folders, then path order.
    found.sort_by(|a, b| {
        source_rank(&a.source)
            .cmp(&source_rank(&b.source))
            .then_with(|| b.mod_count.cmp(&a.mod_count))
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });

    found
}

fn candidate_roots() -> Vec<CandidateRoot> {
    let mut roots = Vec::new();

    // Per-user / launcher-managed locations.
    for variable in [
        "LOCALAPPDATA",
        "APPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "USERPROFILE",
    ] {
        let Some(base) = env::var_os(variable) else {
            continue;
        };
        let base = PathBuf::from(base);

        roots.push(CandidateRoot {
            path: base.join("XXMI Launcher"),
            label: "XXMI Launcher",
        });
        roots.push(CandidateRoot {
            path: base.join("XXMI"),
            label: "XXMI",
        });
        roots.push(CandidateRoot {
            path: base.join("ZZMI"),
            label: "ZZMI",
        });
    }

    // Drive-level installs, C: through Z:. Each probe is a single metadata call.
    for letter in 'C'..='Z' {
        for subdir in [None, Some("Games"), Some("Modding")] {
            for name in ["XXMI", "ZZMI"] {
                let mut path = PathBuf::from(format!("{letter}:\\"));
                if let Some(subdir) = subdir {
                    path.push(subdir);
                }
                path.push(name);

                roots.push(CandidateRoot { path, label: name });
            }
        }
    }

    // Keep only the first occurrence of each candidate root.
    let mut seen = HashSet::new();
    roots.retain(|root| seen.insert(root.path.to_string_lossy().to_lowercase()));
    roots
}

/// XXMI Launcher can keep the per-game loader at `<root>\ZZMI\Mods`; some builds
/// use `<root>\ZZZ\Mods`, and manual installs often put `Mods` at the root.
fn mods_dirs_under(root: &Path) -> [PathBuf; 3] {
    [
        root.join("Mods"),
        root.join("ZZMI").join("Mods"),
        root.join("ZZZ").join("Mods"),
    ]
}

fn source_for(path: &Path) -> &'static str {
    let lower = path.to_string_lossy().to_lowercase();

    if lower.contains("xxmi launcher") {
        "xxmi-launcher"
    } else if lower.contains("xxmi") {
        "xxmi"
    } else if lower.contains("zzmi") {
        "zzmi"
    } else {
        "mods-folder"
    }
}

fn source_rank(source: &str) -> u8 {
    match source {
        "xxmi-launcher" => 0,
        "xxmi" => 1,
        "zzmi" => 2,
        _ => 3,
    }
}
