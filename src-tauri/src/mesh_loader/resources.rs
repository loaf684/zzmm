use std::path::{Path, PathBuf};

use crate::ini_parser::parser::{self, IniLine};

/// One `[Resource*]` section that points at a file we can read.
#[derive(Debug, Clone, Default)]
pub struct IniResource {
    pub section: String,
    pub filename: Option<String>,
    pub stride: Option<usize>,
    pub format: Option<String>,
    /// Directory of the INI: 3DMigoto resolves `filename` relative to it.
    pub base_dir: Option<PathBuf>,
}

impl IniResource {
    pub fn file_stem(&self) -> String {
        let name = self.filename.as_deref().unwrap_or_default();
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        base.rsplit_once('.')
            .map(|(stem, _)| stem.to_string())
            .unwrap_or_else(|| base.to_string())
    }

    pub fn lower_stem(&self) -> String {
        self.file_stem().to_ascii_lowercase()
    }

    pub fn is_index(&self) -> bool {
        let format = self
            .format
            .as_deref()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let stem = self.lower_stem();

        format.contains("UINT")
            || stem.ends_with("ib")
            || stem.ends_with("index")
            || stem.ends_with("indices")
    }

    pub fn is_position(&self) -> bool {
        let stem = self.lower_stem();
        stem.contains("position") || stem.contains("vertex")
    }

    pub fn is_texcoord(&self) -> bool {
        let stem = self.lower_stem();
        stem.contains("texcoord") || stem.contains("uv") || stem.contains("tex")
    }

    /// Component key shared by sibling buffers: `Component1Position` →
    /// `component1`, `BodyTexcoord` → `body`, and shader-dump tags fall away:
    /// `4faabaac-Component1` → `4faabaac`.
    pub fn part_key(&self) -> String {
        let mut key = self.lower_stem();

        for marker in [
            "position",
            "texcoord",
            "texcoords",
            "blend",
            "vertex",
            "indices",
            "index",
            "ib",
            "buf",
            "dds",
        ] {
            if let Some(position) = key.find(marker) {
                key.truncate(position);
                break;
            }
        }

        // Shader-dump mods tag index buffers `…-Component1`; a mod with
        // `Component1`-prefixed names keeps its key (the tag fills the key).
        if let Some(tag) = key.rfind("component") {
            let tail = key[tag + "component".len()..].trim_matches(|c| c == '_' || c == '-');
            if tag > 0 && !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
                key.truncate(tag);
            }
        }

        key.trim_matches(|character: char| character == '_' || character == '-' || character == '.')
            .to_string()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResourceScan {
    pub buffers: Vec<IniResource>,
    pub textures: Vec<IniResource>,
}

/// Collapse type decorations from a resource or component name so related
/// references compare equal: `ResourceAriaKFCTopAIB` → `ariakfctopa` lines up
/// with the `ariakfctop` key of its `AriaKFCTopPosition.buf` sibling.
pub fn component_core(name: &str) -> String {
    let mut core = name.to_ascii_lowercase();

    for prefix in ["textureoverride", "resource"] {
        if let Some(rest) = core.strip_prefix(prefix) {
            if !rest.is_empty() {
                core = rest.to_string();
            }
        }
    }
    core = core
        .trim_start_matches(|c: char| c == '_' || c == '-' || c == '.')
        .to_string();

    for marker in [
        "diffuse",
        "position",
        "texcoords",
        "texcoord",
        "blend",
        "vertex",
        "indices",
        "index",
        "ib",
        "buf",
        "dds",
        "component",
    ] {
        // The marker is a resource suffix (`BodyAIB`, `…-Position`,
        // `…-Component1`). Use the last occurrence: `QingyiBodyAIB` contains
        // `ib` inside `Qingyi` as well, and matching that would reduce both
        // body and hair to `qingy`.
        if let Some(position) = core.rfind(marker) {
            core.truncate(position);
            break;
        }
    }

    // Trailing `componentN` tags: `…-component1`.
    if let Some(tag) = core.rfind("component") {
        if tag > 0 {
            core.truncate(tag);
        }
    }

    let mut core = core
        .trim_matches(|character: char| character == '_' || character == '-' || character == '.');

    // Alias vertex buffers (`ResourceOutfitVB`, `ResourceBellelegsVB`) name
    // the same component as the real position buffer.
    if let Some(stripped) = core.strip_suffix("vb") {
        if !stripped.is_empty() {
            core = stripped.trim_end_matches(|c: char| c == '_' || c == '-' || c == '.');
        }
    }

    core.to_string()
}

/// Loose component identity: `ariakfctop` and `ariakfctopa` (an index
/// buffer's short qualifier) match when one core is a prefix of the other and
/// the remainder is a short qualifier (`_a`, `0`, `bun`).
pub fn cores_match(a: &str, b: &str) -> bool {
    if a == b || a.is_empty() || b.is_empty() {
        return a == b;
    }

    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    long.starts_with(short) && long.len() - short.len() <= 3
}

/// Collect the resource files of an already-decoded INI, so callers that also
/// need the command lines (toggle evaluation) only read the file once.
pub fn scan_lines(lines: &[IniLine], base_dir: Option<&Path>) -> ResourceScan {
    let mut scan = ResourceScan::default();
    let base_dir = base_dir.map(Path::to_path_buf);

    for section in parser::sections(lines) {
        if !section.name.to_ascii_lowercase().starts_with("resource") {
            continue;
        }

        let mut resource = IniResource {
            section: section.name.clone(),
            base_dir: base_dir.clone(),
            ..IniResource::default()
        };

        for (key, value) in parser::section_settings(lines, &section) {
            match key.to_ascii_lowercase().as_str() {
                "filename" => resource.filename = Some(value),
                "stride" => resource.stride = value.trim().parse::<usize>().ok(),
                "format" => resource.format = Some(value),
                _ => {}
            }
        }

        let Some(filename) = resource.filename.as_deref() else {
            continue;
        };
        let lower = filename.to_ascii_lowercase();

        if lower.ends_with(".dds") {
            scan.textures.push(resource);
        } else if lower.ends_with(".ib") {
            // Index buffers can ship bare (`.ib`) instead of inside a `.buf`.
            scan.buffers.push(resource);
        } else if lower.ends_with(".buf") {
            scan.buffers.push(resource);
        }
    }

    scan
}

/// Fallback when no INI describes the buffers: classify `.buf`/`.dds` files by
/// name straight from the mod folder.
pub fn scan_folder(root: &Path) -> ResourceScan {
    let mut scan = ResourceScan::default();
    let mut visited = 0usize;

    fn walk(dir: &Path, depth: usize, visited: &mut usize, scan: &mut ResourceScan) {
        if depth > 4 || *visited > 2000 {
            return;
        }

        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            *visited += 1;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }

            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_dir() {
                walk(&entry.path(), depth + 1, visited, scan);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }

            let lower = name.to_ascii_lowercase();
            if !lower.ends_with(".buf") && !lower.ends_with(".ib") && !lower.ends_with(".dds") {
                continue;
            }

            let resource = IniResource {
                section: "filesystem".to_string(),
                filename: Some(name),
                stride: None,
                format: None,
                base_dir: Some(dir.to_path_buf()),
            };

            if lower.ends_with(".buf") || lower.ends_with(".ib") {
                scan.buffers.push(resource);
            } else {
                scan.textures.push(resource);
            }
        }
    }

    walk(root, 0, &mut visited, &mut scan);
    scan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(filename: &str, format: Option<&str>) -> IniResource {
        IniResource {
            filename: Some(filename.to_string()),
            format: format.map(str::to_string),
            ..IniResource::default()
        }
    }

    #[test]
    fn classifies_buffers() {
        assert!(resource("Component1Position.buf", None).is_position());
        assert!(resource("Component1Texcoord.buf", None).is_texcoord());
        assert!(resource("Component1IB.buf", None).is_index());
        assert!(resource("ib.buf", Some("DXGI_FORMAT_R16_UINT")).is_index());
        assert!(!resource("Component1Position.buf", None).is_index());
    }

    #[test]
    fn derives_shared_part_keys() {
        assert_eq!(
            resource("Component1Position.buf", None).part_key(),
            "component1"
        );
        assert_eq!(resource("Body_Texcoord.buf", None).part_key(), "body");
        assert_eq!(resource("Hair_Texcoord.buf", None).part_key(), "hair");
    }

    #[test]
    fn folder_scan_never_treats_index_buffers_as_textures() {
        let dir = std::env::temp_dir().join(format!("zzmm-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        for name in ["body.ib", "body.buf", "body.dds"] {
            std::fs::write(dir.join(name), b"x").expect("write");
        }

        let scan = scan_folder(&dir);
        assert_eq!(scan.buffers.len(), 2);
        assert_eq!(scan.textures.len(), 1);
        assert_eq!(scan.textures[0].filename.as_deref(), Some("body.dds"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn matches_component_cores() {
        assert_eq!(component_core("ResourceAriaKFCTopAIB"), "ariakfctopa");
        assert_eq!(component_core("ariakfctop"), "ariakfctop");
        assert!(cores_match("ariakfctop", "ariakfctopa"));
        assert!(cores_match("body", "body"));
        // `Qingyi` contains `ib`; the suffix marker must be taken from the end.
        assert_eq!(component_core("ResourceQingyiBodyAIB"), "qingyibodya");
        assert_eq!(component_core("ResourceQingyiHairAIB"), "qingyihaira");
        assert!(!cores_match(
            &component_core("ResourceQingyiBodyAIB"),
            &component_core("ResourceQingyiHairAIB")
        ));
        // Longer remainders are separate components.
        assert!(!cores_match("ari", "ariakfctop"));
        assert!(!cores_match("hair", "hairbunlong"));
    }
}
