//! Reads 3DMigoto's persisted toggle state (`d3dx_user.ini`) and works out
//! which resource sections the mod actually uses with that state.
//!
//! 3DMigoto saves every `global persist $var` to `d3dx_user.ini` next to
//! `d3dx.ini`, namespaced by the mod's deployed path, e.g.
//! `$\mods\<folder>\<file>.ini\swapkey = 1`. Mods with several variants put the
//! alternative resources inside `if $swapkey == 1 … endif` blocks in their
//! `[TextureOverride*]` sections, so evaluating those blocks with the saved
//! values selects exactly the variant the game shows.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::ini_parser::parser::{self, IniLine};
use crate::ini_parser::text;
use crate::mesh_loader::resources;

/// Sentinel setting pushed for a whole-line commented `drawindexed` (for
/// example `; drawindexed = 9123, 0, 0`). `walk` records it against the index
/// buffer bound at that point without treating it as a submitted draw.
const DISABLED_DRAW_MARKER: &str = "__disabled_drawindexed__";

/// Saved variable values for one mod, keyed by the lower-cased variable name.
#[derive(Debug, Default, Clone)]
pub struct ToggleState {
    values: HashMap<String, String>,
    /// True when at least one value actually came from `d3dx_user.ini`.
    pub from_user_config: bool,
}

impl ToggleState {
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn insert(&mut self, name: String, value: String) {
        self.values.insert(name, value);
    }

    /// Seed a preview-only system value (3DMigoto's `DRAW_TYPE`, `$active`)
    /// unless the mod or the saved user config already defines it.
    pub fn set_if_absent(&mut self, name: &str, value: &str) {
        if !self.values.contains_key(name) {
            self.values.insert(name.to_string(), value.to_string());
        }
    }

    /// Force a value, regardless of the INI default or saved state. Used for
    /// the viewer's explicit toggle overrides.
    pub fn set(&mut self, name: &str, value: &str) {
        self.insert(name.to_ascii_lowercase(), value.to_string());
    }

    /// Overlay values saved by 3DMigoto on top of the INI defaults.
    pub fn absorb(&mut self, other: &ToggleState) {
        for (name, value) in &other.values {
            self.values.insert(name.clone(), value.clone());
        }
        self.from_user_config |= other.from_user_config;
    }
}

/// One submitted `drawindexed` sub-range and the colour map that was bound when
/// it was submitted. A section that rebinds the diffuse between draws (a merged
/// hair + cape mod) paints different ranges with different textures, so the
/// range and its colour source travel together instead of collapsing onto the
/// first draw of the section.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawRange {
    pub count: u32,
    pub start: u32,
    /// Resolved colour map resource bound at this draw, if any. `None` means the
    /// draw inherited no diffuse (the game's original texture, or a runtime
    /// capture that has no file on disk).
    pub texture: Option<String>,
}

/// Resource references classified by whether the binding line is active under
/// the saved toggle state.
#[derive(Debug, Default, Clone)]
pub struct Harvest {
    pub active: HashSet<String>,
    pub inactive: HashSet<String>,
    /// Alternative parts the game really skips: inactive vb0 bindings in a
    /// section where another variant *is* active. A binding that is inactive
    /// only because a system value (`DRAW_TYPE`, …) made the branch
    /// unevaluable stays out of this set, so the preview fails open instead of
    /// losing geometry it cannot reason about.
    pub excluded: HashSet<String>,
    /// Position resource → texture resource bound (or resolved) in the same
    /// section.
    pub textures: HashMap<String, String>,
    /// (mesh resource, texture resource) pairs discovered anywhere the two are
    /// used together — including index-buffer sections that only resolve the
    /// `Resource\ZZMI\Diffuse` indirection. Matched by component core.
    pub links: Vec<(String, String)>,
    /// Index-buffer resource → active `drawindexed` sub-draw ranges as
    /// (count, start). Everything outside them is a variant the saved toggle
    /// state does not draw. Each range carries the colour map bound at that
    /// draw so a section that swaps textures mid-list splits cleanly.
    pub draws: HashMap<String, Vec<DrawRange>>,
    /// Index-buffer resource → the position (`vb0`) it is drawn with. One
    /// section may bind several IB/VB pairs in sequence (skin, then outfit),
    /// so this is captured when each draw is submitted, not from the section
    /// as a whole.
    pub index_position: HashMap<String, String>,
    /// Index-buffer resource → the diffuse texture resolved while that IB was
    /// bound. Split IBs (`Body1A`/`Body1B`) can use different colour maps.
    pub index_texture: HashMap<String, String>,
    /// Index-buffer resource → the section that supplied `index_texture`.
    /// A section that also draws another mesh can otherwise leak its diffuse
    /// onto the sibling IB (shader dumps draw several components per section).
    pub index_texture_section: HashMap<String, String>,
    /// Index-buffer resource → `match_first_index`. Sections that share a
    /// first index are alternative resolutions of the same draw; different
    /// values are consecutive slices of one mesh and all belong in the
    /// preview.
    pub index_first: HashMap<String, u32>,
    /// Texture-override section → the resource it supplies (`this = …`,
    /// `Resource\ZZMI\Diffuse`, or `ps-t0`). Texture overrides carry the
    /// component in their section name and often have no mesh binding at all.
    pub named_textures: HashMap<String, String>,
    /// Position resources that appear on a `vb0 =` binding line at all (active
    /// or inactive). A position declared in `[Resource*]` but never bound is a
    /// part the game does not draw (a commented-out/disabled variant), so the
    /// preview must not render it.
    pub bound_positions: HashSet<String>,
    /// Index resources that appear on an `ib =` binding line at all. An index
    /// buffer that is only declared is never submitted.
    pub bound_indices: HashSet<String>,
    /// Resources explicitly cleared with `= null` (`ib = null`). The game
    /// unbinds the slot, so a draw after the clear must not inherit the
    /// previous binding.
    pub cleared: HashSet<String>,
    /// Section → its `hash =` value. 3DMigoto merges every override that
    /// shares a hash, so the hash is how a `handling = skip` in one section
    /// suppresses the draw another section binds.
    pub section_hashes: HashMap<String, String>,
    /// Hash values whose original draw is suppressed (`handling = skip`) for
    /// the current saved toggle state. A conditional skip in a branch the
    /// state does not take is not recorded, so the preview keeps the part.
    pub skipped_hashes: HashSet<String>,
    /// Sections that suppress the original draw but declare no hash (matched
    /// by `match_first_index` alone); tracked separately so the failure mode
    /// stays fail-open when no hash identifies the group.
    pub skipped_sections: HashSet<String>,
    /// Hash values that submit a replacement `drawindexed = auto`: the whole
    /// bound buffer is the draw. S11-style commented draws in a skipped group
    /// have no such line, while Nekomata/Astra/Nicole bind their IB beside one.
    pub auto_draw_hashes: HashSet<String>,
    /// Index-buffer resource → the sections that disable a `drawindexed` for it
    /// with a whole-line comment. A commented draw is the mod's explicit
    /// "this draw is off"; it is only honoured when the section's hash group is
    /// also skipped (see [`Harvest::omits_index_draw`]).
    pub disabled_draw_sections: HashMap<String, BTreeSet<String>>,
}

impl Harvest {
    /// True when the resource sits only in a branch that the saved toggle
    /// state actively excludes.
    pub fn hides(&self, section: &str) -> bool {
        let key = section.to_ascii_lowercase();
        self.excluded.contains(&key) && !self.active.contains(&key)
    }

    pub fn texture_for(&self, position_section: &str) -> Option<&str> {
        self.textures
            .get(&position_section.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn texture_for_index(&self, index_resource: &str) -> Option<&str> {
        self.index_texture
            .get(&index_resource.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn position_for_index(&self, index_resource: &str) -> Option<&str> {
        self.index_position
            .get(&index_resource.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// The sub-draw ranges an index buffer is really submitted with, each with
    /// the colour map bound at that draw.
    pub fn draws_for(&self, index_resource: &str) -> Option<&[DrawRange]> {
        self.draws
            .get(&index_resource.to_ascii_lowercase())
            .map(Vec::as_slice)
    }

    /// True when the INI binds at least one vertex buffer. Mods that never bind
    /// `vb0` (pure shader dumps driven by `draw = ...`) have no binding table,
    /// and the loader must not prune their parts.
    pub fn has_position_bindings(&self) -> bool {
        !self.bound_positions.is_empty()
    }

    pub fn has_index_bindings(&self) -> bool {
        !self.bound_indices.is_empty()
    }

    /// True when a position resource, or a sibling naming the same component,
    /// appears on a `vb0` binding line.
    pub fn binds_position(&self, section: &str) -> bool {
        binds_like(&self.bound_positions, section)
    }

    /// True when an index resource, or a sibling naming the same component,
    /// appears on an `ib` binding line.
    pub fn binds_index(&self, section: &str) -> bool {
        binds_like(&self.bound_indices, section)
    }

    /// True when this section belongs to a hash group whose original draw the
    /// mod suppresses with `handling = skip` for the current toggle state.
    fn skips_original_draw(&self, section: &str) -> bool {
        let key = section.to_ascii_lowercase();
        if self.skipped_sections.contains(&key) {
            return true;
        }
        self.section_hashes
            .get(&key)
            .is_some_and(|hash| self.skipped_hashes.contains(hash))
    }

    /// True when the section's hash group submits a replacement
    /// `drawindexed = auto`, which draws the whole bound buffer.
    fn auto_draws_whole_buffer(&self, section: &str) -> bool {
        self.section_hashes
            .get(&section.to_ascii_lowercase())
            .is_some_and(|hash| self.auto_draw_hashes.contains(hash))
    }

    /// True when a commented-out `drawindexed` for this index buffer is final:
    /// every section that disables a draw for it sits in a hash group whose
    /// original draw the mod suppresses (`handling = skip`) and that submits no
    /// replacement `drawindexed = auto`. The game then draws no geometry here,
    /// so the preview must omit the part instead of falling back to the whole
    /// buffer.
    ///
    /// Fails open: a buffer with no commented draw is never omitted, and a
    /// commented draw in a hash that is not skipped (or that has an `auto`
    /// draw) keeps the whole-buffer fallback because the game still draws it.
    pub fn omits_index_draw(&self, index_resource: &str) -> bool {
        let Some(sections) = self
            .disabled_draw_sections
            .get(&index_resource.to_ascii_lowercase())
        else {
            return false;
        };
        !sections.is_empty()
            && sections.iter().all(|section| {
                self.skips_original_draw(section) && !self.auto_draws_whole_buffer(section)
            })
    }

    /// Merge another INI's harvest: mods may spread their sections over
    /// several files.
    pub fn absorb(&mut self, other: Harvest) {
        self.active.extend(other.active);
        self.inactive.extend(other.inactive);
        self.excluded.extend(other.excluded);
        self.textures.extend(other.textures);
        self.links.extend(other.links);
        self.index_position.extend(other.index_position);
        self.index_texture.extend(other.index_texture);
        self.index_texture_section
            .extend(other.index_texture_section);
        self.index_first.extend(other.index_first);
        self.named_textures.extend(other.named_textures);
        self.bound_positions.extend(other.bound_positions);
        self.bound_indices.extend(other.bound_indices);
        self.cleared.extend(other.cleared);
        self.section_hashes.extend(other.section_hashes);
        self.skipped_hashes.extend(other.skipped_hashes);
        self.skipped_sections.extend(other.skipped_sections);
        self.auto_draw_hashes.extend(other.auto_draw_hashes);
        for (resource, sections) in other.disabled_draw_sections {
            self.disabled_draw_sections
                .entry(resource)
                .or_default()
                .extend(sections);
        }
        for (resource, ranges) in other.draws {
            let entry = self.draws.entry(resource).or_default();
            for range in ranges {
                if !entry
                    .iter()
                    .any(|existing| existing.count == range.count && existing.start == range.start)
                {
                    entry.push(range);
                }
            }
        }
    }
}

/// Match a resource against a set of bound names, falling back to component
/// identity so an alias (`ResourceOutfitVB` bound for `ResourceOutfitPosition`)
/// still counts as bound. Fail open whenever an unparseable name is involved so
/// the preview never drops a part it cannot reason about.
fn binds_like(bound: &HashSet<String>, section: &str) -> bool {
    let key = section.to_ascii_lowercase();
    if bound.contains(&key) {
        return true;
    }

    let core = resources::component_core(section);
    if core.is_empty() {
        return true;
    }

    for candidate in bound {
        if candidate == &key {
            return true;
        }
        let other = resources::component_core(candidate);
        if other.is_empty() || resources::cores_match(&other, &core) {
            return true;
        }
    }

    false
}

/// Look for `d3dx_user.ini` next to `d3dx.ini` (the loader/importer directory).
/// `dirs` is ordered: mod folder, Mods folder, its parent (the loader root).
pub fn read_toggle_state(dirs: &[PathBuf], folder_name: &str) -> ToggleState {
    let mut state = ToggleState::default();
    let folder = folder_name.trim().to_ascii_lowercase();
    let folder_underscored = folder.replace(' ', "_");
    let folder_hyphened = folder.replace(' ', "-");

    let paths: Vec<PathBuf> = dirs
        .iter()
        .flat_map(|dir| ["d3dx_user.ini", "3DUser.ini"].map(|name| dir.join(name)))
        .collect();

    for path in paths {
        if !path.is_file() {
            continue;
        }

        let Ok((content, _)) = text::read(&path) else {
            continue;
        };

        for line in content.lines() {
            let Some((lhs, value)) = split_assignment(line) else {
                continue;
            };
            if !lhs.starts_with('$') {
                continue;
            }

            let normalized = lhs.replace('/', "\\").to_ascii_lowercase();
            let Some(name) = normalized.rsplit('\\').next() else {
                continue;
            };
            let name = name.trim().to_string();
            if name.is_empty() {
                continue;
            }

            let value = value.trim().trim_matches('"').to_string();

            // Only values whose namespace names this mod's folder are ours.
            // Matching by variable name alone would pick up another mod's
            // state, so a renamed folder simply means "no saved state".
            if !folder.is_empty()
                && (normalized.contains(&folder)
                    || normalized.contains(&folder_underscored)
                    || normalized.contains(&folder_hyphened))
            {
                state.insert(name, value);
                state.from_user_config = true;
            }
        }
    }

    state
}

/// True when this INI declares `$name` in a `[Constants]` block, i.e. it is the
/// file whose namespace the loader persists the variable under.
pub fn declares_constant(lines: &[IniLine], name: &str) -> bool {
    let wanted = name.trim_start_matches('$').to_ascii_lowercase();
    for section in parser::sections(lines) {
        if !section.name.eq_ignore_ascii_case("constants") {
            continue;
        }
        for index in section.header + 1..section.end {
            let text = strip_comment(&lines[index].text);
            let Some((lhs, _)) = split_assignment(&text) else {
                continue;
            };
            if let Some(variable) = lhs.rsplit('$').next() {
                if lhs.contains('$') && variable.trim().to_ascii_lowercase() == wanted {
                    return true;
                }
            }
        }
    }
    false
}

/// The key 3DMigoto writes a persisted variable under: the loader's `\mods`
/// prefix, the mod folder, the INI that declares it, then the variable.
/// `$\mods\sunna\resources\sunna.ini\jacket = 1`
pub fn user_config_key(folder: &str, ini_relative: &str, variable: &str) -> String {
    let folder = folder.trim().to_ascii_lowercase().replace('/', "\\");
    let ini = ini_relative.trim().to_ascii_lowercase().replace('/', "\\");
    let variable = variable.trim().trim_start_matches('$').to_ascii_lowercase();
    format!("$\\mods\\{folder}\\{ini}\\{variable}")
}

/// Set one persisted value in the loader's user-ini text, adding the line under
/// `[Constants]` when it is not there yet. Returns `None` when nothing changed,
/// so a save with no effective change does not rewrite the file.
pub fn write_user_value(content: &str, key: &str, value: &str) -> Option<String> {
    let wanted = key.to_ascii_lowercase();
    // 3DMigoto writes CRLF. Rebuild with whatever the file already uses so a
    // save never rewrites 872 line endings just to change one value.
    let newline = if content.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut replaced = false;

    for line in &mut lines {
        let Some((lhs, _)) = split_assignment(line) else {
            continue;
        };
        if lhs.trim().to_ascii_lowercase() == wanted {
            let next = format!("{key} = {value}");
            if line.trim() == next {
                return None;
            }
            *line = next;
            replaced = true;
            break;
        }
    }

    if !replaced {
        let entry = format!("{key} = {value}");
        match lines
            .iter()
            .position(|line| line.trim().eq_ignore_ascii_case("[constants]"))
        {
            Some(header) => lines.insert(header + 1, entry),
            None => {
                if !lines.is_empty() && !lines.last().is_some_and(|line| line.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push("[Constants]".to_string());
                lines.push(entry);
            }
        }
    }

    let mut out = lines.join(newline);
    if content.ends_with('\n') {
        out.push_str(newline);
    }
    Some(out)
}

/// `[Constants]` declarations: variables and their in-game default values.
pub fn collect_defaults(lines: &[IniLine], state: &mut ToggleState) {
    for section in parser::sections(lines) {
        if !section.name.eq_ignore_ascii_case("constants") {
            continue;
        }

        for index in section.header + 1..section.end {
            let Some((lhs, value)) = split_assignment(&lines[index].text) else {
                continue;
            };
            if !lhs.contains('$') {
                continue;
            }

            let name = lhs
                .rsplit('$')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }

            if state.value(&name).is_none() {
                let value = value.trim().trim_matches('"').to_string();
                state.insert(name, value);
            }
        }
    }
}

/// Replay the plain assignments in `[Present]`.
///
/// 3DMigoto runs `[Present]` once per frame, and menu-driven mods use it to
/// derive the toggles that guard their `drawindexed` lines (`$yifu0`, `$hair1`,
/// `$shenti0`, …) from persistent selector variables (`$yifu`, `$hair`). Those
/// derived values are never written to `d3dx_user.ini`, so without this replay
/// they stay at their `[Constants]` default of `0` and the preview draws only
/// the always-on props instead of the outfit the game shows.
///
/// Only assignments that can be evaluated exactly are applied: a literal, or a
/// copy of another variable, inside the branch that is active under the current
/// state. Time-, cursor- and input-dependent expressions (and arithmetic) are
/// skipped so the preview stays a deterministic snapshot.
pub fn apply_present_assignments(lines: &[IniLine], state: &mut ToggleState) {
    for section in parser::sections(lines) {
        if !section.name.eq_ignore_ascii_case("present") {
            continue;
        }

        let mut frames: Vec<Frame> = Vec::new();

        for index in section.header + 1..section.end {
            let text = strip_comment(&lines[index].text);
            if text.is_empty() {
                continue;
            }

            let (word, rest) = split_word(text);
            match word.to_ascii_lowercase().as_str() {
                "if" => {
                    let taken = evaluate(rest, state).unwrap_or(false);
                    frames.push(Frame {
                        any_taken: taken,
                        active: taken,
                    });
                    continue;
                }
                "elif" => {
                    if let Some(frame) = frames.last_mut() {
                        let taken = !frame.any_taken && evaluate(rest, state).unwrap_or(false);
                        frame.active = taken;
                        frame.any_taken |= taken;
                    }
                    continue;
                }
                "else" => {
                    // `else if $swapvar == 2` is an `elif` in disguise.
                    let condition = {
                        let trimmed =
                            rest.trim_start_matches(|c: char| c == '=' || c.is_whitespace());
                        trimmed
                            .strip_prefix("if")
                            .map(|condition| condition.trim_start())
                    };
                    if let Some(frame) = frames.last_mut() {
                        let taken = !frame.any_taken
                            && match condition {
                                Some(condition) => evaluate(condition, state).unwrap_or(false),
                                None => true,
                            };
                        frame.active = taken;
                        frame.any_taken |= taken;
                    }
                    continue;
                }
                "endif" => {
                    frames.pop();
                    continue;
                }
                _ => {}
            }

            if frames.iter().any(|frame| !frame.active) {
                continue;
            }

            let Some((lhs, value)) = split_assignment(text) else {
                continue;
            };
            let lhs = lhs.trim();
            if !lhs.starts_with('$') {
                // `post $active = 0` and other deferred writes are not part of
                // the state while the frame is being drawn.
                continue;
            }

            let name = lhs
                .rsplit('$')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            let Some(value) = simple_assignment_value(value, state) else {
                continue;
            };
            state.insert(name, value);
        }
    }
}

/// Evaluate the right-hand side of a `[Present]` assignment when it is a
/// literal or a copy of another variable. Returns `None` for values that need a
/// numeric expression parser (`$initFrame + 1`, `time - $ts`, …): guessing them
/// would make the preview less accurate, not more.
fn simple_assignment_value(value: &str, state: &ToggleState) -> Option<String> {
    let value = value.trim();

    if let Some(rest) = value.strip_prefix('$') {
        let name = rest
            .rsplit('\\')
            .next()
            .unwrap_or(rest)
            .trim()
            .to_ascii_lowercase();
        return state.value(&name).map(str::to_string);
    }

    if value.parse::<f64>().is_ok() {
        return Some(value.to_string());
    }

    let quoted = (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
        || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2);
    if quoted {
        return Some(value[1..value.len() - 1].to_string());
    }

    None
}

/// Walk every section's command list, follow `run = CommandList…` calls, and
/// record, with the saved values, which resources are used, which alternatives
/// are excluded, what texture each mesh component resolves to, and which
/// sub-draw ranges of an index buffer the game actually submits.
pub fn harvest(lines: &[IniLine], state: &ToggleState) -> Harvest {
    let mut out = Harvest::default();

    // First pass: every section's stripped setting lines, indexed by
    // lower-cased name, so `run` calls resolve no matter the file order. A
    // BTreeMap keeps the evaluation order deterministic.
    let mut sections: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for section in parser::sections(lines) {
        let lower = section.name.to_ascii_lowercase();
        let mut settings: Vec<String> = Vec::new();
        for index in section.header + 1..section.end {
            let raw = &lines[index].text;
            // `strip_comment` removes `;` and `；` comments but not `#`, so
            // test the raw line first: a whole-line commented draw must reach
            // the walker as a marker rather than being dropped or misparsed.
            if is_disabled_drawindexed(raw) {
                settings.push(DISABLED_DRAW_MARKER.to_string());
                continue;
            }
            let text = strip_comment(raw);
            if !text.is_empty() {
                settings.push(text.to_string());
            }
        }
        sections.entry(lower).or_default().extend(settings);
    }

    // Second pass: the sections that drive mesh draws. Resource declarations,
    // hotkeys and constants never bind meshes or textures.
    for (lower, settings) in &sections {
        if lower.starts_with("resource") || lower.starts_with("key") || lower == "constants" {
            continue;
        }

        let mut run = SectionRun {
            section: lower.clone(),
            ..SectionRun::default()
        };
        let mut visited = HashSet::new();
        walk(
            settings,
            state,
            &sections,
            true,
            &mut visited,
            0,
            &mut run,
            &mut out,
        );
        fold(&run, &mut out);
    }

    out
}

#[derive(Debug, Default)]
struct SectionRun {
    /// (slot key, resource name, active) for every vb/ib/texture binding.
    mesh: Vec<(String, String, bool)>,
    /// `Resource\ZZMI\Diffuse` targets resolved through `run` chains, in file
    /// order — the diffuse decides the colours the game actually paints.
    diffuse: Vec<String>,
    /// One active `drawindexed` line with the buffers/textures bound at that
    /// moment. Ranges must never be attributed to a different IB than the one
    /// the section had bound when it submitted the draw.
    draws: Vec<Draw>,
    /// Index buffers that had *any* `drawindexed` line, including lines inside
    /// branches the toggle state does not take. An IB in this set with no
    /// active ranges is a part the game submits no geometry for; it must not
    /// fall back to drawing the whole buffer.
    drawn_ibs: std::collections::BTreeSet<String>,
    current_ib: Option<String>,
    current_position: Option<String>,
    current_diffuse: Option<String>,
    current_ps0: Option<String>,
    /// (slot, resource) of the last active colour binding on a slot other than
    /// `ps-t0` — in ZZZ that is the `ps-t3` diffuse. Mods that swap the colour
    /// map between sub-draws (Seed paints the scooter with `che.dds`, then the
    /// body with `body.dds`) rely on this being captured at the draw itself.
    current_color: Option<(String, String)>,
    /// True when the last index binding was `ib = null`, so a draw submitted
    /// before a new index is bound draws nothing of ours.
    ib_cleared: bool,
    match_first: u32,
    section: String,
    /// `hash =` of this section, lower-cased. Groups every override 3DMigoto
    /// merges for one draw.
    hash: Option<String>,
    /// An active `handling = skip`: the game's original draw is suppressed.
    skip_original: bool,
    /// An active `drawindexed = auto`: the whole bound buffer is redrawn in
    /// place of the suppressed original.
    auto_draw: bool,
    /// Index buffers whose `drawindexed` line this section comments out. Kept
    /// apart from `drawn_ibs`: a commented draw only removes the part when the
    /// hash group is skipped (see `omits_index_draw`).
    disabled_ibs: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct Draw {
    ib: Option<String>,
    position: Option<String>,
    diffuse: Option<String>,
    ps0: Option<String>,
    /// Colour texture bound to a non-`ps-t0` slot (ZZZ's `ps-t3`) when this
    /// draw was submitted.
    color: Option<String>,
    count: u32,
    start: u32,
    /// The index slot was explicitly cleared before this draw.
    ib_cleared: bool,
}

/// Evaluate one section's command list under `gate` (the caller's active
/// state), following nested command lists and collecting bindings, resolved
/// diffuse indirections and active sub-draw ranges into `run`.
fn walk(
    settings: &[String],
    state: &ToggleState,
    sections: &BTreeMap<String, Vec<String>>,
    gate: bool,
    visited: &mut HashSet<String>,
    depth: usize,
    run: &mut SectionRun,
    out: &mut Harvest,
) {
    let mut frames: Vec<Frame> = Vec::new();
    let active = |frames: &[Frame]| gate && frames.iter().all(|frame| frame.active);

    for text in settings {
        let (word, rest) = split_word(text);

        match word.to_ascii_lowercase().as_str() {
            "if" => {
                let taken = evaluate(rest, state).unwrap_or(false);
                frames.push(Frame {
                    any_taken: taken,
                    active: taken,
                });
                continue;
            }
            "elif" => {
                if let Some(frame) = frames.last_mut() {
                    let taken = !frame.any_taken && evaluate(rest, state).unwrap_or(false);
                    frame.active = taken;
                    frame.any_taken |= taken;
                }
                continue;
            }
            "else" => {
                // `else if $swapvar == 2` chains: the branch must evaluate its
                // own condition, the same way `elif` does.
                let condition = {
                    let trimmed = rest.trim_start_matches(|c: char| c == '=' || c.is_whitespace());
                    if trimmed.starts_with("if") {
                        Some(trimmed[2..].trim_start())
                    } else if trimmed.is_empty() {
                        None
                    } else {
                        None
                    }
                };
                if let Some(frame) = frames.last_mut() {
                    let taken = !frame.any_taken
                        && match condition {
                            Some(condition) => evaluate(condition, state).unwrap_or(false),
                            None => true,
                        };
                    frame.active = taken;
                    frame.any_taken |= taken;
                }
                continue;
            }
            "endif" => {
                frames.pop();
                continue;
            }
            "run" => {
                let rest = rest.trim_start_matches(|c: char| c == '=' || c.is_whitespace());
                if depth >= 4 || !active(&frames) {
                    continue;
                }
                let Some(target) = command_list_name(rest) else {
                    continue;
                };
                if !visited.insert(target.clone()) {
                    continue;
                }
                if let Some(callee) = sections.get(&target) {
                    walk(callee, state, sections, true, visited, depth + 1, run, out);
                }
                continue;
            }
            DISABLED_DRAW_MARKER => {
                // A whole-line comment `; drawindexed = ...` is the draw the
                // mod switched off. Record it without adding a range; the part
                // is only dropped when the hash group is skipped.
                if let Some(ib) = run.current_ib.clone() {
                    run.disabled_ibs.insert(ib);
                }
                continue;
            }
            "drawindexed" => {
                let rest = rest.trim_start_matches(|c: char| c == '=' || c.is_whitespace());
                if let Some(ib) = run.current_ib.clone() {
                    // Even a draw in a branch the toggles disable proves this
                    // IB is an explicit, range-driven part rather than an
                    // implicit `drawindexed = auto` buffer.
                    run.drawn_ibs.insert(ib);
                }
                if active(&frames) {
                    // `drawindexed = auto` redraws the whole bound buffer; it
                    // has no numeric range, but it *is* a submitted draw, so a
                    // skipped hash group that carries one must keep its part.
                    if rest.eq_ignore_ascii_case("auto") {
                        run.auto_draw = true;
                    } else if let Some((count, start)) = parse_drawindexed(rest) {
                        run.draws.push(Draw {
                            ib: run.current_ib.clone(),
                            position: run.current_position.clone(),
                            diffuse: run.current_diffuse.clone(),
                            ps0: run.current_ps0.clone(),
                            color: run
                                .current_color
                                .as_ref()
                                .map(|(_, resource)| resource.clone()),
                            count,
                            start,
                            ib_cleared: run.ib_cleared,
                        });
                    }
                }
                continue;
            }
            _ => {}
        }

        let Some((key, value)) = split_assignment(text) else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();

        // `hash =` names the draw call this override replaces; `handling =
        // skip` suppresses that draw. 3DMigoto merges every override sharing a
        // hash into one group, so a skip on the group's `...IB` section removes
        // the original draw that a sibling `...A` section binds an IB for.
        if key == "hash" {
            if active(&frames) {
                let hash = value.trim().to_ascii_lowercase();
                if !hash.is_empty() && hash != "null" {
                    run.hash = Some(hash);
                }
            }
            continue;
        }
        if key == "handling" {
            if active(&frames) && value.trim().eq_ignore_ascii_case("skip") {
                run.skip_original = true;
            }
            continue;
        }

        // `Resource\ZZMI\Diffuse = ref ResourceBody.Base`: the game's diffuse
        // slot, resolved by the importer through `run` command lists. The
        // `ZZMI` namespace also accepts the short aliases `D`/`N`/`L`/`M`:
        // `SetTex` maps `ResourceD` onto the diffuse before `SetTextures`
        // binds it (Belle, and mods generated by older importers).
        if key == r"resource\zzmi\diffuse" || key == r"resource\zzmi\d" {
            if active(&frames) {
                if let Some(resource) = resource_name(value) {
                    run.diffuse.push(resource.clone());
                    run.current_diffuse = Some(resource.clone());
                    out.active.insert(resource);
                }
            }
            continue;
        }

        if key == "match_first_index" {
            if let Ok(first) = value.trim().parse::<u32>() {
                run.match_first = first;
            }
            continue;
        }

        // `this = ResourceBody.Diffuse` replaces the texture the game matched.
        // It is the shader-dump/Anby convention for colour maps and never
        // appears on an `ib`/`vb0` line, so record it as the section's texture.
        if key == "this" {
            if let Some(resource) = resource_name(value) {
                if active(&frames) {
                    run.current_ps0 = Some(resource.clone());
                    out.active.insert(resource);
                }
            }
            continue;
        }

        if slot_kind(&key).is_none() {
            continue;
        }
        let Some(resource) = resource_name(value) else {
            // `ib = null` / `vb0 = null` explicitly unbinds the slot. Clear the
            // running binding so a later `drawindexed` in the same section is
            // not attributed to the previous mesh (a disabled part would
            // otherwise inherit its sibling's geometry).
            if active(&frames) && clears_binding(value) {
                match slot_kind(&key) {
                    Some(Slot::Position) => run.current_position = None,
                    Some(Slot::Index) => {
                        run.current_ib = None;
                        run.ib_cleared = true;
                    }
                    Some(Slot::Texture) => {
                        if key == "ps-t0" {
                            run.current_ps0 = None;
                        } else if run
                            .current_color
                            .as_ref()
                            .is_some_and(|(slot, _)| *slot == key)
                        {
                            run.current_color = None;
                        }
                    }
                    _ => {}
                }
                out.cleared.insert(key.clone());
            }
            continue;
        };

        let effective = active(&frames);
        if effective {
            out.active.insert(resource.clone());
            match slot_kind(&key) {
                Some(Slot::Position) => run.current_position = Some(resource.clone()),
                Some(Slot::Index) => {
                    run.current_ib = Some(resource.clone());
                    run.ib_cleared = false;
                }
                Some(Slot::Texture) if key == "ps-t0" => run.current_ps0 = Some(resource.clone()),
                Some(Slot::Texture) => {
                    if looks_like_data_texture(&resource) || looks_like_null_texture(&resource) {
                        if run
                            .current_color
                            .as_ref()
                            .is_some_and(|(slot, _)| *slot == key)
                        {
                            run.current_color = None;
                        }
                    } else {
                        run.current_color = Some((key.clone(), resource.clone()));
                    }
                }
                _ => {}
            }
        } else {
            out.inactive.insert(resource.clone());
        }
        run.mesh.push((key, resource, effective));
    }
}

/// Fold one evaluated section into the harvest.
fn fold(run: &SectionRun, out: &mut Harvest) {
    let mut positions: Vec<(&str, bool)> = Vec::new();
    let mut indexes: Vec<(&str, bool)> = Vec::new();
    let mut textures: Vec<(&str, &str, bool)> = Vec::new();

    for (key, resource, active) in &run.mesh {
        match slot_kind(key) {
            Some(Slot::Position) => positions.push((resource.as_str(), *active)),
            Some(Slot::Index) => indexes.push((resource.as_str(), *active)),
            Some(Slot::Texture) => textures.push((key.as_str(), resource.as_str(), *active)),
            Some(Slot::Vertex) => {}
            None => {}
        }
    }

    // Any binding line — even one inside a false branch — proves the INI knows
    // how to submit that resource. The preview uses this to tell a
    // declared-but-never-bound part (e.g. Pulchra Body2, whose position and
    // index overrides are commented out) from a live one.
    for (resource, _) in &positions {
        out.bound_positions.insert((*resource).to_string());
    }
    for (resource, _) in &indexes {
        out.bound_indices.insert((*resource).to_string());
    }

    // Record the draw-call group (`hash =`) and whether this section suppresses
    // the group's original draw. A hash shared by an `...IB` section
    // (`handling = skip`, maybe `drawindexed = auto`) and an `...A` section
    // (`ib = Resource...`) is one group in 3DMigoto.
    if let Some(hash) = &run.hash {
        out.section_hashes.insert(run.section.clone(), hash.clone());
        if run.skip_original {
            out.skipped_hashes.insert(hash.clone());
        }
        if run.auto_draw {
            out.auto_draw_hashes.insert(hash.clone());
        }
    } else if run.skip_original {
        out.skipped_sections.insert(run.section.clone());
    }

    // Sections that comment out a draw for an index buffer. Kept separate from
    // the submitted ranges: the part is only dropped when this section's hash
    // group is skipped and submits no `auto` draw (S11's bag, Soukaku's mask).
    for ib in &run.disabled_ibs {
        out.disabled_draw_sections
            .entry(ib.clone())
            .or_default()
            .insert(run.section.clone());
    }

    // A real variant toggle: the game draws exactly one of the alternatives a
    // section binds, so the others are excluded. Bindings without an active
    // sibling (unevaluable `DRAW_TYPE` frames, textures of dead sections) stay
    // renderable.
    if positions.iter().any(|(_, active)| *active) {
        for (resource, active) in &positions {
            if !*active {
                out.excluded.insert((*resource).to_string());
            }
        }
    }

    // The texture this section paints itself with. The resolved game diffuse
    // slot wins (it carries the colour/variant logic), then an explicit
    // `ps-t0`, then a lone texture binding.
    let texture: Option<String> = run
        .diffuse
        .first()
        .cloned()
        .or_else(|| run.current_ps0.clone())
        .or_else(|| {
            let bound: Vec<(&str, &str)> = textures
                .iter()
                .filter(|(_, _, active)| *active)
                .map(|(key, resource, _)| (*key, *resource))
                .collect();
            if bound.is_empty() {
                return None;
            }
            if let Some((_, resource)) = bound.iter().find(|(key, _)| *key == "ps-t0") {
                return Some((*resource).to_string());
            }
            // The last active binding that is not a data map is the colour the
            // game paints with: ZZZ shaders put the diffuse on `ps-t3` and
            // rebind it per sub-draw, so the final active binding wins. A
            // normal/light/specular map is never a colour source, so a lone
            // data map still yields no texture.
            bound
                .iter()
                .rev()
                .find(|(_, resource)| {
                    !looks_like_data_texture(resource) && !looks_like_null_texture(resource)
                })
                .map(|(_, resource)| (*resource).to_string())
        })
        // `ResourceNullTexture` is a 1×1 placeholder that disables a slot; it is
        // never a colour map. Dropping it here stops a texture-only section in a
        // second INI from overwriting the real diffuse the mesh INI supplied
        // (Ye's ponytail A declares its colours in `0.ini` and binds a null
        // glow slot in `10YE.ini`).
        .filter(|resource| !looks_like_null_texture(resource));

    // A section may draw several IB/VB pairs one after another (Yuzuha draws
    // skin, then swaps to the outfit buffer). Attribute every range to the IB
    // and VB that were bound when that specific draw was submitted.
    let active_index: Option<&str> = indexes
        .iter()
        .find(|(_, active)| *active)
        .map(|(resource, _)| *resource);
    let active_position: Option<&str> = {
        let mut active = positions.iter().filter(|(_, active)| *active);
        match (active.next(), active.next()) {
            (Some((resource, _)), None) => Some(*resource),
            _ => None,
        }
    };

    for ib in &run.drawn_ibs {
        out.draws.entry(ib.clone()).or_default();
    }

    for draw in &run.draws {
        // A draw submitted after `ib = null` binds no index buffer; do not let
        // it fall back to the section's first active index and clone its
        // geometry onto a disabled part.
        if draw.ib.is_none() && draw.ib_cleared {
            continue;
        }
        let Some(ib) = draw.ib.as_deref().or(active_index) else {
            continue;
        };
        // The colour map bound at this specific draw: an explicit diffuse, the
        // `this`/`ps-t0` target, then the section's resolved colour. A data map
        // or null placeholder is never a colour source.
        let draw_texture = draw
            .diffuse
            .clone()
            .or_else(|| draw.ps0.clone())
            .or_else(|| draw.color.clone())
            .or_else(|| texture.clone())
            .filter(|resource| {
                !looks_like_null_texture(resource) && !looks_like_data_texture(resource)
            });
        out.draws
            .entry(ib.to_string())
            .or_default()
            .push(DrawRange {
                count: draw.count,
                start: draw.start,
                texture: draw_texture,
            });
        out.index_first
            .entry(ib.to_string())
            .or_insert(run.match_first);
        if let Some(position) = draw.position.as_deref().or(active_position) {
            out.index_position
                .entry(ib.to_string())
                .or_insert_with(|| position.to_string());
        }
        if let Some(texture) = draw
            .diffuse
            .as_deref()
            .or(draw.ps0.as_deref())
            .or(draw.color.as_deref())
            .or(texture.as_deref())
        {
            out.index_texture
                .entry(ib.to_string())
                .or_insert_with(|| texture.to_string());
            out.index_texture_section
                .entry(ib.to_string())
                .or_insert_with(|| run.section.clone());
        }
    }

    // Sections that only rebind an IB (`TextureOverrideNekomataHairA`) rely on
    // the game's `drawindexed = auto`: the whole buffer is the draw. Record
    // their bindings so the loader can render them even without explicit
    // ranges, and so `match_first_index` grouping still applies.
    for (resource, active) in &indexes {
        if !*active {
            continue;
        }
        out.index_first
            .entry((*resource).to_string())
            .or_insert(run.match_first);
        if let Some(position) = active_position {
            // Only a section with a single IB can lend its lone `vb0` to that
            // IB. A section that swaps several IBs (body A/B, skin/outfit)
            // binds the last VB for the last draw; borrowing it for the first
            // buffer pairs body geometry with legs/outfit positions.
            let index_count = indexes.iter().filter(|(_, active)| *active).count();
            if index_count == 1 {
                out.index_position
                    .entry((*resource).to_string())
                    .or_insert_with(|| position.to_string());
            }
        }
        if let Some(texture) = &texture {
            out.index_texture
                .entry((*resource).to_string())
                .or_insert_with(|| texture.clone());
            out.index_texture_section
                .entry((*resource).to_string())
                .or_insert_with(|| run.section.clone());
        }
    }

    // Remember which texture this override supplies, even for texture-only
    // sections (`TextureOverride…Diffuse`). The loader matches it to a mesh
    // component by section name.
    if let Some(texture) = &texture {
        if !run.section.is_empty() {
            out.named_textures
                .insert(run.section.clone(), texture.clone());
        }
    }

    // Direct association: the position drawn by this section.
    if let Some(texture) = &texture {
        for (resource, active) in &positions {
            if *active {
                out.textures
                    .entry((*resource).to_string())
                    .or_insert_with(|| texture.clone());
            }
        }
    }

    // Loose association facts for every mesh resource this section couples
    // with the texture: neighbour sections that bind only
    // `ib = ResourceAriaKFCTopAIB` get their colours from `run` chains here.
    if let Some(texture) = &texture {
        for (resource, _) in positions.iter().chain(indexes.iter()) {
            let link = ((*resource).to_string(), texture.clone());
            if !out.links.contains(&link) {
                out.links.push(link);
            }
        }
    }
}

/// A 1×1 placeholder texture that disables a slot. 3DMigoto mods declare these
/// (`ResourceNullTexture`) and bind them to unused `ps-tN` slots; they carry no
/// colour and must not be mistaken for a diffuse map.
fn looks_like_null_texture(resource: &str) -> bool {
    resource.to_ascii_lowercase().contains("null")
}

/// Menu art a mod binds for its own on-screen UI. Rebinding it changes what the
/// menu looks like, never what the character renders, so it must not make a
/// menu flag look like a mesh control (the slot menu's `$menu`, `$oiled`,
/// `$nails` all rebind `ps-t100` art this way).
fn looks_like_ui_texture(resource: &str) -> bool {
    [
        "menuitem",
        "menubg",
        "menu.",
        "renderimage",
        "credit",
        "preview",
        "slotitem",
        "slotbg",
        "slider",
        "cursor",
        "hover",
    ]
    .iter()
    .any(|marker| resource.to_ascii_lowercase().contains(marker))
}

fn looks_like_data_texture(resource: &str) -> bool {
    [
        "normal",
        "light",
        "materialmap",
        "mask",
        "specular",
        "roughness",
        "metalness",
        "emissive",
        "glow",
        "heightmap",
        "opacity",
        "alpha",
    ]
    .iter()
    .any(|marker| resource.to_ascii_lowercase().contains(marker))
}

/// `drawindexed = 7173, 0, 0` → `Some((7173, 0))` as (count, start). Lines
/// like `drawindexed = auto` do not parse and are ignored.
fn parse_drawindexed(rest: &str) -> Option<(u32, u32)> {
    let mut parts = rest.split(',');
    let count = parts.next()?.trim().parse::<u32>().ok()?;
    if count == 0 {
        return None;
    }
    let start = parts
        .next()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(0);

    Some((count, start))
}

/// `run = CommandListBodyADiffuse` → `commandlistbodyadiffuse`. `null` clears
/// nothing here and is not a section call.
///
/// 3DMigoto allows `run` to target any section, not only names that start with
/// `CommandList`. Seed's outfit passes are named
/// `CustomShaderTransparency_*`; rejecting them drops the cloth ranges while
/// keeping the skin ranges, so the preview shows a nude body. The caller
/// already checks the section table, so unknown targets are harmless.
fn command_list_name(value: &str) -> Option<String> {
    let cleaned = value.split(',').next().unwrap_or(value);
    let cleaned = cleaned.trim().trim_matches('"').trim();
    let lower = cleaned.to_ascii_lowercase();
    if lower.is_empty() || lower == "null" {
        return None;
    }

    Some(lower)
}

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Position,
    Vertex,
    Index,
    Texture,
}

/// Classify a binding key: `vb0` positions the mesh, `vb1`-`vb3` carry skin /
/// texcoord / blend data, `ib` indexes it, texture slots paint it.
fn slot_kind(key: &str) -> Option<Slot> {
    match key {
        "vb0" => Some(Slot::Position),
        "vb1" | "vb2" | "vb3" => Some(Slot::Vertex),
        "ib" => Some(Slot::Index),
        key if key.starts_with("ps-t") || key.starts_with("vs-t") => Some(Slot::Texture),
        _ => None,
    }
}

struct Frame {
    /// A branch of this `if` chain has already matched.
    any_taken: bool,
    /// The current branch runs.
    active: bool,
}

/// Remove a trailing `;` comment from a settings line.
fn strip_comment(line: &str) -> &str {
    let cut = line.find([';', '；']).unwrap_or(line.len());
    line[..cut].trim()
}

/// True when the whole line is a comment that disables a `drawindexed` call:
/// `; drawindexed = 9123, 0, 0`, `# drawindexed = 3`, `；drawindexed = 3`.
/// The ini parser keeps comments, so the raw text still carries the command;
/// the preview honours it only when the section's hash group is skipped.
fn is_disabled_drawindexed(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(body) = trimmed
        .strip_prefix(';')
        .or_else(|| trimmed.strip_prefix('#'))
        .or_else(|| trimmed.strip_prefix('；'))
    else {
        return false;
    };

    body.trim_start()
        .split(|character: char| character.is_whitespace() || character == '=')
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("drawindexed"))
}

fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(';') {
        return None;
    }

    let (key, rest) = trimmed.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }

    let value = rest.split(';').next().unwrap_or(rest).trim();
    Some((key, value))
}

fn split_word(text: &str) -> (&str, &str) {
    match text.find(char::is_whitespace) {
        Some(at) => (&text[..at], text[at..].trim()),
        None => (text, ""),
    }
}

/// True when a binding value clears its slot (`ib = null`, `vb0 = ref null`).
fn clears_binding(value: &str) -> bool {
    let mut name = value.trim();
    let (word, tail) = split_word(name);
    if word.eq_ignore_ascii_case("ref") || word.eq_ignore_ascii_case("copy") {
        name = tail;
    }
    let last = name
        .split(['\\', '/'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .next_back()
        .unwrap_or("");
    last.eq_ignore_ascii_case("null")
}

/// `vb0 = ResourceEllenHair` or `ps-t0 = ref ResourceBody.Base` →
/// `ellenhair` / `resourcebody.base`: a leading `ref`/`copy` transfer prefix
/// is stripped before reading the target.
fn resource_name(value: &str) -> Option<String> {
    let mut name = value.trim();
    let (word, tail) = split_word(name);
    if word.eq_ignore_ascii_case("ref") || word.eq_ignore_ascii_case("copy") {
        name = tail;
    }

    let segments: Vec<&str> = name
        .split(['\\', '/'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect();
    let last = *segments.last()?;

    if last.is_empty() || last.eq_ignore_ascii_case("null") {
        return None;
    }

    let is_resource = segments
        .iter()
        .any(|segment| segment.eq_ignore_ascii_case("resource"))
        || last.to_ascii_lowercase().starts_with("resource");
    if !is_resource {
        return None;
    }

    Some(last.to_ascii_lowercase())
}

#[derive(Debug, Clone)]
enum Token {
    Var(String),
    Number(f64),
    Text(String),
    Word,
    Op(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Number(f64),
    Text(String),
}

impl Value {
    fn truthy(&self) -> bool {
        match self {
            Value::Number(number) => *number != 0.0,
            Value::Text(text) => !text.is_empty() && text != "0",
        }
    }

    fn as_text(&self) -> String {
        match self {
            Value::Number(number) => number.to_string(),
            Value::Text(text) => text.clone(),
        }
    }
}

fn tokenize(expr: &str) -> Vec<Token> {
    let chars: Vec<char> = expr.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;

    while index < chars.len() {
        let current = chars[index];

        if current.is_whitespace() || current == ',' {
            index += 1;
            continue;
        }

        if current == '$' {
            index += 1;
            let start = index;
            while index < chars.len()
                && !chars[index].is_whitespace()
                && !"()!<>=&|,".contains(chars[index])
            {
                index += 1;
            }
            let raw: String = chars[start..index].iter().collect();
            let name = raw
                .rsplit('\\')
                .next()
                .unwrap_or(&raw)
                .trim()
                .to_ascii_lowercase();
            tokens.push(Token::Var(name));
            continue;
        }

        if current == '"' || current == '\'' {
            index += 1;
            let start = index;
            while index < chars.len() && chars[index] != current {
                index += 1;
            }
            tokens.push(Token::Text(chars[start..index].iter().collect()));
            index += 1;
            continue;
        }

        if current == '(' || current == ')' {
            tokens.push(Token::Op(current.to_string()));
            index += 1;
            continue;
        }

        if "!<>=&|".contains(current) {
            let pair: String = chars[index..(index + 2).min(chars.len())].iter().collect();
            let two = ["==", "!=", ">=", "<=", "&&", "||"];
            if two.contains(&pair.as_str()) {
                tokens.push(Token::Op(pair));
                index += 2;
                continue;
            }
            if current == '&' || current == '|' {
                tokens.push(Token::Op(
                    if current == '&' { "&&" } else { "||" }.to_string(),
                ));
            } else {
                tokens.push(Token::Op(current.to_string()));
            }
            index += 1;
            continue;
        }

        let start = index;
        while index < chars.len()
            && !chars[index].is_whitespace()
            && !"()!<>=&|,".contains(chars[index])
        {
            index += 1;
        }
        let raw: String = chars[start..index].iter().collect();

        match raw.parse::<f64>() {
            Ok(number) => tokens.push(Token::Number(number)),
            Err(_) => {
                let _ = raw;
                tokens.push(Token::Word);
            }
        }
    }

    tokens
}

/// Evaluate one `if` expression. `None` means "cannot tell", and callers fail
/// open (treat the branch as taken) so a preview never loses parts because of
/// an expression this parser does not understand.
pub fn evaluate(expr: &str, state: &ToggleState) -> Option<bool> {
    let tokens = tokenize(expr);
    if tokens.is_empty() {
        return None;
    }

    let mut parser = ExprParser {
        tokens,
        index: 0,
        state,
    };
    parser.parse_or()
}

struct ExprParser<'a> {
    tokens: Vec<Token>,
    index: usize,
    state: &'a ToggleState,
}

impl ExprParser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.index)
    }

    fn take(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.index).cloned();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    fn take_op(&mut self, wanted: &str) -> bool {
        match self.peek() {
            Some(Token::Op(op)) if op == wanted => {
                self.index += 1;
                true
            }
            _ => false,
        }
    }

    fn parse_or(&mut self) -> Option<bool> {
        let mut value = self.parse_and();

        while self.take_op("||") {
            let right = self.parse_and();
            value = match (value, right) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            };
        }

        value
    }

    fn parse_and(&mut self) -> Option<bool> {
        let mut value = self.parse_unary();

        while self.take_op("&&") {
            let right = self.parse_unary();
            value = match (value, right) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            };
        }

        value
    }

    fn parse_unary(&mut self) -> Option<bool> {
        if self.take_op("!") {
            return self.parse_unary().map(|value| !value);
        }

        if self.take_op("(") {
            let value = self.parse_or();
            self.take_op(")");
            return value;
        }

        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Option<bool> {
        let left = self.parse_operand()?;
        let Some(Token::Op(op)) = self.peek().cloned() else {
            return Some(left.truthy());
        };

        if !["==", "!=", ">=", "<=", ">", "<"].contains(&op.as_str()) {
            return Some(left.truthy());
        }

        self.index += 1;
        let right = self.parse_operand()?;

        match (left, right) {
            (Value::Number(a), Value::Number(b)) => Some(match op.as_str() {
                "==" => a == b,
                "!=" => a != b,
                ">" => a > b,
                "<" => a < b,
                ">=" => a >= b,
                _ => a <= b,
            }),
            (left, right) => {
                let a = left.as_text().to_ascii_lowercase();
                let b = right.as_text().to_ascii_lowercase();
                Some(match op.as_str() {
                    "==" => a == b,
                    "!=" => a != b,
                    ">" => a > b,
                    "<" => a < b,
                    ">=" => a >= b,
                    _ => a <= b,
                })
            }
        }
    }

    fn parse_operand(&mut self) -> Option<Value> {
        match self.take()? {
            Token::Number(number) => Some(Value::Number(number)),
            Token::Text(text) => Some(Value::Text(text)),
            Token::Var(name) => {
                // A variable with no saved value and no `[Constants]` default
                // is treated as off (0), so `if $var == 1` branches are not
                // taken by default and the preview does not render every
                // variant just because a setting is missing from the INI.
                let raw = self.state.value(&name).unwrap_or("0");
                match raw.parse::<f64>() {
                    Ok(number) => Some(Value::Number(number)),
                    Err(_) => Some(Value::Text(raw.to_string())),
                }
            }
            // System values (`DRAW_TYPE`, `cursor_showing`, ...) are unknown, so
            // the comparison stays undecided and the caller fails open.
            Token::Word | Token::Op(_) => None,
        }
    }
}

/// Variables a `[Key*]` section binds to a key: the `$name = ...` option list
/// written inside the section (or any assignment 3DMigoto key types use).
///
/// This is the user-facing definition of a toggle. A `global persist $name` in
/// `[Constants]`, assignments in `[Present]` / command lists, or condition-only
/// flags are internal state, even when they change the rendered mesh.
pub fn key_toggle_variables(lines: &[IniLine]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for section in parser::sections(lines) {
        if !section.name.to_ascii_lowercase().starts_with("key") {
            continue;
        }
        for index in section.header + 1..section.end {
            let Some((lhs, _)) = split_assignment(&lines[index].text) else {
                continue;
            };
            let Some(variable) = lhs.trim().strip_prefix('$') else {
                continue;
            };
            let variable = variable.trim().to_ascii_lowercase();
            if !variable.is_empty() {
                names.insert(variable);
            }
        }
    }
    names
}

/// Variables a mod's own slot menu drives, which no `[Key*]` section mentions:
/// an assignment whose right-hand side reads the variable it assigns
/// (`$jacket = 1 - $jacket`, `$page = $page + 1`). That idiom only makes sense
/// for a control a user flips; a flag the mod derives from game state is
/// assigned a literal or another variable's value, so it stays out of the
/// preview's controls.
pub fn self_toggled_variables(lines: &[IniLine]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for section in parser::sections(lines) {
        for index in section.header + 1..section.end {
            let text = strip_comment(&lines[index].text);
            let Some((lhs, rhs)) = split_assignment(&text) else {
                continue;
            };
            // `[Constants]` writes `global persist $var = ...` under any number
            // of leading keywords; the tail after the last `$` is the name.
            if !lhs.contains('$') {
                continue;
            }
            let variable = lhs
                .rsplit('$')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            if variable.is_empty() {
                continue;
            }
            if reads_variable(rhs, &variable) {
                names.insert(variable);
            }
        }
    }
    names
}

/// True when `expression` reads `$name` as a whole variable rather than as the
/// prefix of a longer one (`$page` must not match `$pagecount`).
fn reads_variable(expression: &str, name: &str) -> bool {
    let lower = expression.to_ascii_lowercase();
    let needle = format!("${name}");
    let mut from = 0;

    while let Some(found) = lower[from..].find(&needle) {
        let after = from + found + needle.len();
        let is_whole = lower[after..]
            .chars()
            .next()
            .is_none_or(|character| !(character.is_ascii_alphanumeric() || character == '_'));
        if is_whole {
            return true;
        }
        from = after;
    }

    false
}

/// Variable names used as operands inside `if` / `elif` / `else if`
/// conditions. Only these can gate a binding, so only these can change what
/// the preview draws.
pub fn condition_variables(lines: &[IniLine]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for section in parser::sections(lines) {
        for index in section.header + 1..section.end {
            let text = strip_comment(&lines[index].text);
            if text.is_empty() {
                continue;
            }
            let (word, rest) = split_word(text);
            match word.to_ascii_lowercase().as_str() {
                "if" | "elif" => collect_condition_vars(rest, &mut names),
                "else" => {
                    let condition = rest.trim_start_matches(|c: char| c == '=' || c.is_whitespace());
                    if let Some(condition) = condition.strip_prefix("if") {
                        collect_condition_vars(condition, &mut names);
                    }
                }
                _ => {}
            }
        }
    }
    names
}

fn collect_condition_vars(expr: &str, out: &mut BTreeSet<String>) {
    for token in tokenize(expr) {
        if let Token::Var(name) = token {
            if !name.is_empty() {
                out.insert(name);
            }
        }
    }
}

/// Numeric literal values observed for one variable: `$name = <n>` assignments
/// (in `[Constants]`, `[Key*]` option lists, `[Present]`, ...) and
/// `$name == <n>` / `$name != <n>` comparisons in `if` conditions. Values are
/// normalized (`1.0` → `1`) and sorted, so the same constant written two ways
/// collapses to one option.
pub fn candidate_values(lines: &[IniLine], name: &str) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    for section in parser::sections(lines) {
        for index in section.header + 1..section.end {
            let text = strip_comment(&lines[index].text);
            if text.is_empty() {
                continue;
            }
            let (word, rest) = split_word(text);
            match word.to_ascii_lowercase().as_str() {
                "if" | "elif" => collect_compared_values(rest, name, &mut values),
                "else" => {
                    let condition = rest.trim_start_matches(|c: char| c == '=' || c.is_whitespace());
                    if let Some(condition) = condition.strip_prefix("if") {
                        collect_compared_values(condition, name, &mut values);
                    }
                }
                _ => {
                    let Some((lhs, value)) = split_assignment(text) else {
                        continue;
                    };
                    let lhs = lhs.trim();
                    // `[Constants]` writes `global persist $var = n`; every
                    // other section writes the bare `$var = n`.
                    if !lhs.contains('$') {
                        continue;
                    }
                    let variable = lhs
                        .rsplit('$')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase();
                    if variable == name {
                        // `[Key*]` option lists write `$var = 0, 1, 2`; a
                        // single assignment writes just the one value.
                        for part in value.split(',') {
                            if let Some(number) = normalize_number(part) {
                                values.insert(number);
                            }
                        }
                    }
                }
            }
        }
    }
    values
}

fn collect_compared_values(expr: &str, name: &str, out: &mut BTreeSet<String>) {
    let tokens = tokenize(expr);
    for window in tokens.windows(3) {
        if let [Token::Var(variable), Token::Op(op), Token::Number(number)] = window {
            if variable == name && (op == "==" || op == "!=") {
                out.insert(format_number(*number));
            }
        }
    }
}

/// `1.0` / `"1"` → `Some("1")`; anything non-numeric → `None`.
fn normalize_number(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('"').trim_matches('\'');
    trimmed.parse::<f64>().ok().map(format_number)
}

/// Normalize a numeric literal (`1.0` → `1`); `None` when it is not a number.
/// Exposed so the preview can test whether a toggle's live value is discrete.
pub fn numeric_literal(value: &str) -> Option<String> {
    normalize_number(value)
}

fn format_number(number: f64) -> String {
    if number.fract() == 0.0 && number.abs() < 1e15 {
        format!("{}", number as i64)
    } else {
        number.to_string()
    }
}

/// Canonical fingerprint of everything a toggle can change about the rendered
/// model: which renderable resources are bound on an active path, the colour
/// map resolved for each part and index buffer, and the draw ranges the game
/// submits (with the colour map of each range).
///
/// Data maps (normal / light / specular / mask / glow / ...) and 1×1 null
/// placeholders are deliberately excluded: the viewer never renders them and
/// the loader never uses them as a colour source, so a toggle that only
/// rebinds one of those slots cannot change the preview and must not earn a
/// control. Two toggle states with the same fingerprint render the same model.
pub fn harvest_signature(harvest: &Harvest) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(
        harvest.active.len() + harvest.textures.len() + harvest.index_texture.len() + harvest.draws.len(),
    );

    let hidden = |resource: &str| {
        looks_like_data_texture(resource)
            || looks_like_null_texture(resource)
            || looks_like_ui_texture(resource)
    };

    for name in &harvest.active {
        if hidden(name) {
            continue;
        }
        out.push(format!("a:{name}"));
    }
    for (position, texture) in &harvest.textures {
        if hidden(texture) {
            continue;
        }
        out.push(format!("t:{position}:{texture}"));
    }
    for (index, texture) in &harvest.index_texture {
        if hidden(texture) {
            continue;
        }
        out.push(format!("i:{index}:{texture}"));
    }
    for (index, ranges) in &harvest.draws {
        for range in ranges {
            // The geometry of a draw is always part of the fingerprint; only the
            // colour map is dropped when it is menu art, so a flag that merely
            // switches the mod's on-screen menu cannot look like a mesh change.
            let texture = range
                .texture
                .as_deref()
                .filter(|texture| !hidden(texture))
                .unwrap_or("");
            out.push(format!(
                "d:{index}:{}:{}:{texture}",
                range.count, range.start
            ));
        }
    }

    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(pairs: &[(&str, &str)]) -> ToggleState {
        let mut state = ToggleState::default();
        for (name, value) in pairs {
            state.insert(name.to_string(), value.to_string());
        }
        state
    }

    fn lines(text: &str) -> Vec<IniLine> {
        parser::split_lines(text)
    }

    fn range(count: u32, start: u32) -> DrawRange {
        DrawRange {
            count,
            start,
            texture: None,
        }
    }

    #[test]
    fn collects_variables_used_in_conditions() {
        let ini = lines(
            "[CommandListBody]\r\n\
             if $swapkey == 1\r\n\
             \tvb0 = ResourceBodyAlt\r\n\
             elif $color != 0\r\n\
             \tvb0 = ResourceBodyTint\r\n\
             endif\r\n\
             [CommandListHair]\r\n\
             else if $hair == 2\r\n\
             \tvb0 = ResourceHairAlt\r\n\
             endif\r\n",
        );

        let names = condition_variables(&ini);
        assert!(names.contains("swapkey"));
        assert!(names.contains("color"));
        assert!(names.contains("hair"));
        // `ResourceBodyAlt` is a binding target, not a condition operand.
        assert!(!names.contains("resourcebodyalt"));
    }

    #[test]
    fn collects_numeric_candidate_values_from_assignments_and_comparisons() {
        let ini = lines(
            "[Constants]\r\n\
             global persist $swapkey = 0\r\n\
             global $mode = 2.0\r\n\
             [KeyToggle]\r\n\
             key = VK_1\r\n\
             type = toggle\r\n\
             $swapkey = 1\r\n\
             $swapkey = 2\r\n\
             $name = \"hair\"\r\n\
             [TextureOverrideBody]\r\n\
             if $swapkey == 3\r\n\
             \tvb0 = ResourceBodyPos3\r\n\
             endif\r\n",
        );

        assert_eq!(
            candidate_values(&ini, "swapkey"),
            ["0", "1", "2", "3"].iter().map(|v| v.to_string()).collect()
        );
        // Fractional and quoted values normalize; only numbers count.
        assert_eq!(
            candidate_values(&ini, "mode"),
            ["2".to_string()].into_iter().collect()
        );
        assert!(candidate_values(&ini, "name").is_empty());
    }

    #[test]
    fn only_key_sections_declare_toggle_variables() {
        let ini = lines(
            "[Constants]\r\n\
             global persist $shoes = 0\r\n\
             global persist $bear = 0\r\n\
             [KeyShoes]\r\n\
             key = VK_1\r\n\
             type = cycle\r\n\
             $shoes = 0, 1\r\n\
             [TextureOverrideBear]\r\n\
             if $bear == 1\r\n\
             \tvb0 = ResourceBear\r\n\
             endif\r\n\
             [KeyHair]\r\n\
             key = VK_2\r\n\
             type = cycle\r\n\
             $hair = 0, 1\r\n\
             $hair = 2\r\n",
        );

        let declared = key_toggle_variables(&ini);
        assert!(declared.contains("shoes"));
        assert!(declared.contains("hair"));
        // `[Constants]` persistence and a condition-only flag are not controls.
        assert!(!declared.contains("bear"));
    }

    #[test]
    fn self_toggled_variables_reads_the_slot_menu_idiom() {
        let ini = lines(
            "[Constants]\r\n\
             global persist $jacket = 1\r\n\
             global persist $pagecount = 3\r\n\
             global $bear = 0\r\n\
             [CommandListClickedSlot]\r\n\
             $jacket = 1 - $jacket\r\n\
             $pagecount = $pagecount + 0\r\n\
             $bear = 1\r\n\
             [CommandListDimensions]\r\n\
             $width = $pagecount + 1\r\n\
             $page = $pagecount + 1\r\n",
        );

        let driven = self_toggled_variables(&ini);
        assert!(driven.contains("jacket"));
        assert!(driven.contains("pagecount"));
        // Assigned from another variable's value, not from itself.
        assert!(!driven.contains("width"));
        // Assigned a literal: a flag the mod derives, not a control.
        assert!(!driven.contains("bear"));
        // `$page` must not match the `$pagecount` on the right-hand side: a
        // name is read whole, never as a prefix of a longer variable.
        assert!(!driven.contains("page"));
    }

    #[test]
    fn writes_a_persisted_toggle_in_the_loaders_namespace() {
        let key = user_config_key("Sunna RaceQueen", "resources\\Sunna.ini", "$jacket");
        assert_eq!(key, "$\\mods\\sunna racequeen\\resources\\sunna.ini\\jacket");

        let content = "; header\n[Constants]\n$\\mods\\other\\x.ini\\jacket = 1\n";
        let updated = write_user_value(&content, &key, "0").expect("changed");
        assert!(updated.contains("; header"), "keeps comments");
        assert!(
            updated.contains("$\\mods\\other\\x.ini\\jacket = 1"),
            "keeps another mod's value"
        );
        assert!(
            updated.contains(&format!("{key} = 0")),
            "adds the value under [Constants]"
        );

        // Writing the same value again is a no-op, so a save does not rewrite
        // the file for nothing.
        assert!(write_user_value(&updated, &key, "0").is_none());

        // An existing line is replaced in place, whatever its spacing.
        let again = write_user_value(&updated, &key, "1").expect("changed");
        assert!(again.contains(&format!("{key} = 1")));
        assert!(!again.contains(&format!("{key} = 0")));
    }

    #[test]
    fn finds_the_ini_that_declares_a_constant() {
        let declaring = lines(
            "[Constants]\r\n\
             global persist $jacket = 1\r\n",
        );
        let other = lines(
            "[Constants]\r\n\
             global persist $hat = 0\r\n",
        );
        assert!(declares_constant(&declaring, "jacket"));
        assert!(declares_constant(&declaring, "$Jacket"));
        assert!(!declares_constant(&other, "jacket"));
    }

    #[test]
    fn harvest_signature_tracks_which_variant_is_drawn() {
        let ini = lines(
            "[TextureOverrideBody]\r\n\
             hash = abc\r\n\
             if $swapkey == 0\r\n\
             \tvb0 = ResourceBodyBase\r\n\
             \tib = ResourceBodyBaseIB\r\n\
             \tdrawindexed = 30, 0, 0\r\n\
             else\r\n\
             \tvb0 = ResourceBodyAlt\r\n\
             \tib = ResourceBodyAltIB\r\n\
             \tdrawindexed = 60, 0, 0\r\n\
             endif\r\n",
        );

        let base = harvest(&ini, &state(&[("swapkey", "0")]));
        let alt = harvest(&ini, &state(&[("swapkey", "1")]));
        assert_ne!(harvest_signature(&base), harvest_signature(&alt));

        // A variable nothing on screen depends on leaves the fingerprint alone.
        let other = harvest(&ini, &state(&[("swapkey", "0"), ("unused", "1")]));
        assert_eq!(harvest_signature(&base), harvest_signature(&other));
    }

    #[test]
    fn collects_comma_separated_option_lists() {
        let ini = lines(
            "[KeyCycle]\r\n\
             key = VK_1\r\n\
             type = cycle\r\n\
             $swapkey = 0, 1, 2\r\n",
        );

        assert_eq!(
            candidate_values(&ini, "swapkey"),
            ["0", "1", "2"].iter().map(|v| v.to_string()).collect()
        );
    }

    #[test]
    fn harvest_signature_ignores_data_map_only_changes() {
        let ini = lines(
            "[TextureOverrideBody]\r\n\
             hash = abc\r\n\
             vb0 = ResourceBodyPosition\r\n\
             ps-t0 = ResourceBodyDiffuse\r\n\
             if $glow == 1\r\n\
             \tps-t9 = ResourceBodyNormalMap\r\n\
             endif\r\n",
        );

        let base = harvest(&ini, &state(&[("glow", "0")]));
        let glow = harvest(&ini, &state(&[("glow", "1")]));
        // The normal map is bound on an active path but never rendered.
        assert!(glow.active.contains("resourcebodynormalmap"));
        assert_eq!(harvest_signature(&base), harvest_signature(&glow));
    }

    #[test]
    fn harvest_signature_tracks_a_colour_map_swap() {
        let ini = lines(
            "[TextureOverrideBody]\r\n\
             hash = abc\r\n\
             vb0 = ResourceBodyPosition\r\n\
             if $color == 0\r\n\
             \tps-t0 = ResourceBodyDiffuseA\r\n\
             else\r\n\
             \tps-t0 = ResourceBodyDiffuseB\r\n\
             endif\r\n",
        );

        let a = harvest(&ini, &state(&[("color", "0")]));
        let b = harvest(&ini, &state(&[("color", "1")]));
        assert_ne!(harvest_signature(&a), harvest_signature(&b));
    }

    #[test]
    fn set_overrides_any_existing_value() {
        let mut state = state(&[("swapkey", "0")]);
        state.set("SwapKey", "2");
        assert_eq!(state.value("swapkey"), Some("2"));
    }

    #[test]
    fn evaluates_simple_comparisons() {
        let state = state(&[("swapkey", "1")]);

        assert_eq!(evaluate("$swapkey == 1", &state), Some(true));
        assert_eq!(evaluate("$swapkey != 1", &state), Some(false));
        assert_eq!(evaluate("$swapkey", &state), Some(true));
        // A variable with no saved value defaults to off (0).
        assert_eq!(evaluate("$missing == 1", &state), Some(false));
        // System values expressed as bare words cannot be evaluated.
        assert_eq!(evaluate("DRAW_TYPE == 1", &state), None);
    }

    #[test]
    fn evaluates_boolean_operators() {
        let state = state(&[("a", "1"), ("b", "0")]);

        assert_eq!(evaluate("$a == 1 && $b == 0", &state), Some(true));
        assert_eq!(evaluate("$a == 2 || $b == 0", &state), Some(true));
        assert_eq!(evaluate("$a == 1 && $b == 1", &state), Some(false));
        assert_eq!(evaluate("!$b", &state), Some(true));
    }

    #[test]
    fn replays_present_derived_toggles() {
        // A menu-driven mod: the persistent selectors have defaults, but the
        // `drawindexed` guards are only set by `[Present]`.
        let ini = lines(
            "[Constants]\r\n\
             global persist $outfit = 1\r\n\
             global persist $sleeves = 0\r\n\
             global persist $sleeves_on = 0\r\n\
             global persist $sleeves_off = 0\r\n\
             \r\n\
             [Present]\r\n\
             if $outfit == 1\r\n\
             \t$sleeves_on = 1\r\n\
             \t$sleeves_off = 0\r\n\
             elif $outfit == 2\r\n\
             \t$sleeves_on = 0\r\n\
             \t$sleeves_off = 1\r\n\
             endif\r\n\
             if $sleeves == 0\r\n\
             \t$sleeves_on = 0\r\n\
             endif\r\n\
             $dt = time - $ts\r\n\
             post $outfit = 0\r\n\
             \r\n\
             [TextureOverrideBody]\r\n\
             hash = abc\r\n\
             ib = ResourceBody\r\n\
             drawindexed = 3, 0, 0\r\n\
             if $sleeves_off == 1\r\n\
             \tdrawindexed = 6, 3, 0\r\n\
             endif\r\n",
        );

        let mut state = ToggleState::default();
        collect_defaults(&ini, &mut state);
        apply_present_assignments(&ini, &mut state);

        assert_eq!(state.value("sleeves_on"), Some("0"));
        assert_eq!(state.value("sleeves_off"), Some("0"));
        // `post` writes happen after the frame; they must not leak in.
        assert_eq!(state.value("outfit"), Some("1"));
        // Time arithmetic is not guessed.
        assert_eq!(state.value("dt"), None);

        let harvested = harvest(&ini, &state);
        let ranges = harvested.draws_for("resourcebody").expect("draw ranges");
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].count, 3);
    }

    #[test]
    fn hides_resources_only_bound_in_false_branches() {
        let state = state(&[("swapkey", "0")]);
        let ini = lines(
            "[TextureOverrideBody]\r\n\
             hash = abc\r\n\
             vb0 = ResourceBodyBase\r\n\
             ps-t0 = ResourceBodyBaseDiffuse\r\n\
             if $swapkey == 1\r\n\
             \tvb0 = ResourceBodyAlt\r\n\
             \tps-t0 = ResourceBodyAltDiffuse\r\n\
             endif\r\n",
        );

        let harvest = harvest(&ini, &state);
        assert!(harvest.active.contains("resourcebodybase"));
        assert!(harvest.inactive.contains("resourcebodyalt"));
        assert!(harvest.hides("ResourceBodyAlt"));
        assert!(!harvest.hides("ResourceBodyBase"));
        assert_eq!(
            harvest.texture_for("ResourceBodyBase"),
            Some("resourcebodybasediffuse")
        );
    }

    #[test]
    fn evaluates_else_if_chains() {
        let state = state(&[("swapvar", "3")]);
        let ini = lines(
            "[CommandListPosition]\r\n\
             if $swapvar == 0\r\n\
             \tvb0 = ResourceBodyPos0\r\n\
             else if $swapvar == 1\r\n\
             \tvb0 = ResourceBodyPos1\r\n\
             else if $swapvar == 3\r\n\
             \tvb0 = ResourceBodyPos3\r\n\
             else\r\n\
             \tvb0 = ResourceBodyPosElse\r\n\
             endif\r\n",
        );

        let harvest = harvest(&ini, &state);
        assert!(harvest.active.contains("resourcebodypos3"));
        assert!(harvest.hides("ResourceBodyPos0"));
        assert!(harvest.hides("ResourceBodyPos1"));
        // The tail `else` is never reached because the third branch matched.
        assert!(!harvest.active.contains("resourcebodyposelse"));
    }

    #[test]
    fn keeps_parts_when_the_variant_condition_is_unevaluable() {
        let state = state(&[("swapkey", "0")]);
        let ini = lines(
            "[TextureOverrideBody]\r\n\
             hash = abc\r\n\
             if DRAW_TYPE == 2\r\n\
             \tvb1 = ResourceBodyTexcoord\r\n\
             elif DRAW_TYPE == 1\r\n\
             \tvb0 = ResourceBodyAlt\r\n\
             endif\r\n",
        );

        let harvest = harvest(&ini, &state);
        // `DRAW_TYPE` is a system value: the branch is unknown, so the
        // bindings sit in `inactive` but are never excluded.
        assert!(harvest.inactive.contains("resourcebodyalt"));
        assert!(!harvest.excluded.contains("resourcebodyalt"));
        assert!(!harvest.hides("ResourceBodyAlt"));
    }

    #[test]
    fn links_index_sections_to_the_resolved_diffuse() {
        let state = state(&[("color", "0")]);
        let ini = lines(
            "[CommandListBodyDiffuse]\r\n\
             if $color == 0\r\n\
             \tResource\\ZZMI\\Diffuse = ref ResourceBody.Base\r\n\
             endif\r\n\
             [TextureOverrideBodyA]\r\n\
             hash = abc\r\n\
             ib = ResourceBodyAIB\r\n\
             run = CommandListBodyDiffuse\r\n",
        );

        let harvest = harvest(&ini, &state);
        assert!(harvest.links.contains(&(
            "resourcebodyaib".to_string(),
            "resourcebody.base".to_string()
        )));
    }

    #[test]
    fn resolves_the_zzmi_short_diffuse_alias() {
        let ini = lines(
            "[TextureOverrideBellelegsA]\r\n\
             ib = ResourceBellelegsAIB\r\n\
             Resource\\ZZMI\\D = ref ResourceBellebodyADiffuse\r\n\
             run = CommandListBellelegsdraw\r\n\
             [CommandListBellelegsdraw]\r\n\
             drawindexed = 3, 0, 0\r\n",
        );

        let harvest = harvest(&ini, &ToggleState::default());
        assert_eq!(
            harvest.texture_for_index("ResourceBellelegsAIB"),
            Some("resourcebellebodyadiffuse")
        );
    }

    #[test]
    fn treats_a_null_placeholder_as_no_colour() {
        // Ye binds a 1×1 `ResourceNullTexture` to the unused `ps-t9` glow slot in
        // the mesh INI while the same section name gets its real diffuse from the
        // texture-only INI. The placeholder must not be recorded, or it would
        // overwrite the good diffuse when the two harvests are merged.
        let texture_only = lines(
            "[TextureOverride马尾A]\r\n\
             Resource\\ZZMI\\Diffuse = ref ResourceDiffuseB\r\n",
        );
        let mesh = lines(
            "[TextureOverride马尾A]\r\n\
             ib = Resource马尾AIB\r\n\
             ps-t9 = ResourceNullTexture\r\n\
             drawindexed = 3, 0, 0\r\n",
        );

        let mut merged = harvest(&texture_only, &ToggleState::default());
        merged.absorb(harvest(&mesh, &ToggleState::default()));

        assert_eq!(
            merged
                .named_textures
                .get("textureoverride马尾a")
                .map(String::as_str),
            Some("resourcediffuseb")
        );
        // A lone placeholder never becomes an index colour either.
        assert_eq!(merged.texture_for_index("Resource马尾AIB"), None);
    }

    #[test]
    fn records_the_section_that_supplied_each_index_colour_map() {
        let ini = lines(
            "[TextureOverride981]\r\n\
             ib = Resource981AIB\r\n\
             drawindexed = 3, 0, 0\r\n\
             [TextureOverrideD800]\r\n\
             vb0 = ResourceD800Position\r\n\
             ib = ResourceD800AIB\r\n\
             ps-t0 = ResourceD800Diffuse\r\n\
             drawindexed = 3, 0, 0\r\n",
        );

        let harvest = harvest(&ini, &ToggleState::default());
        assert_eq!(
            harvest.texture_for_index("ResourceD800AIB"),
            Some("resourced800diffuse")
        );
        assert_eq!(
            harvest
                .index_texture_section
                .get("resourced800aib")
                .map(String::as_str),
            Some("textureoverrided800")
        );
        // A section with no texture binding must not claim one.
        assert_eq!(harvest.texture_for_index("Resource981AIB"), None);
    }

    #[test]
    fn picks_the_last_colour_binding_on_any_texture_slot() {
        let ini = lines(
            // Seed binds its diffuse on `ps-t3` and rebinds it per sub-draw;
            // the final active binding is the colour the game paints with.
            "[TextureOverrideSeedBody]\r\n\
             ib = ResourceSeedBodyIB\r\n\
             ps-t3 = ResourceSeedSwimsuit3\r\n\
             ps-t3 = ResourceSeedSwimsuit4\r\n\
             ps-t4 = ResourceSeedBodyNormal\r\n\
             ps-t5 = ResourceSeedBodyLight\r\n\
             drawindexed = 3, 0, 0\r\n\
             [TextureOverrideSeedHair]\r\n\
             ib = ResourceSeedHairIB\r\n\
             ps-t4 = ResourceSeedHairNormal\r\n\
             ps-t5 = ResourceSeedHairLight\r\n\
             drawindexed = 3, 0, 0\r\n",
        );

        let harvest = harvest(&ini, &ToggleState::default());
        assert_eq!(
            harvest.texture_for_index("ResourceSeedBodyIB"),
            Some("resourceseedswimsuit4")
        );
        // A lone data map is never a colour source.
        assert_eq!(harvest.texture_for_index("ResourceSeedHairIB"), None);
    }

    #[test]
    fn records_the_colour_map_bound_at_each_draw() {
        // A merged mod draws the third-party hair, then switches to a captured
        // game texture for the cape draw, then switches back. Each range must
        // carry the texture that was bound when it was submitted.
        let ini = lines(
            "[TextureOverrideHair]\r\n\
             ib = ResourceHairAIB\r\n\
             Resource\\ZZMI\\Diffuse = ref ResourceHairDiffuse\r\n\
             drawindexed = 100, 0, 0\r\n\
             Resource\\ZZMI\\Diffuse = ref ResourceOGHairDiffuse\r\n\
             drawindexed = 50, 100, 0\r\n\
             Resource\\ZZMI\\Diffuse = ref ResourceHairDiffuse\r\n\
             drawindexed = 25, 150, 0\r\n",
        );

        let harvest = harvest(&ini, &ToggleState::default());
        let ranges = harvest.draws_for("ResourceHairAIB").expect("ranges");
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].texture.as_deref(), Some("resourcehairdiffuse"));
        assert_eq!(ranges[1].texture.as_deref(), Some("resourceoghairdiffuse"));
        assert_eq!(ranges[2].texture.as_deref(), Some("resourcehairdiffuse"));
    }

    #[test]
    fn records_the_ps_t3_colour_bound_at_each_draw() {
        // Seed swaps `ps-t3` between sub-draws: the scooter is painted with
        // `che.dds`, then the body with `body.dds`. Each range must keep the
        // map that was bound when it was submitted instead of inheriting the
        // section's final binding.
        let ini = lines(
            "[TextureOverrideVehicle]\r\n\
             ib = ResourceVehicleIB\r\n\
             ps-t3 = ResourceCheDiffuse\r\n\
             drawindexed = 19425, 561612, 0\r\n\
             ps-t3 = ResourceBodyDiffuse\r\n\
             ps-t4 = ResourceBodyNormal\r\n\
             drawindexed = 503742, 0, 0\r\n\
             drawindexed = 57870, 503742, 0\r\n",
        );

        let harvest = harvest(&ini, &ToggleState::default());
        let ranges = harvest.draws_for("ResourceVehicleIB").expect("ranges");
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].texture.as_deref(), Some("resourcechediffuse"));
        assert_eq!(ranges[1].texture.as_deref(), Some("resourcebodydiffuse"));
        assert_eq!(ranges[2].texture.as_deref(), Some("resourcebodydiffuse"));
    }

    #[test]
    fn follows_run_targets_without_a_commandlist_prefix() {
        // Seed submits its cloth ranges from `CustomShaderTransparency_*`
        // sections. Only accepting `CommandList*` targets silently dropped
        // every outfit draw while keeping the body's skin draws.
        let ini = lines(
            "[TextureOverrideBody]\r\n\
             ib = ResourceBodyIB\r\n\
             ps-t3 = ResourceBodyDiffuse\r\n\
             drawindexed = 3, 0, 0\r\n\
             run = CustomShaderTransparency_14\r\n\
             [CustomShaderTransparency_14]\r\n\
             blend = ADD BLEND_FACTOR INV_BLEND_FACTOR\r\n\
             handling = skip\r\n\
             drawindexed = 25884, 973287, 0\r\n",
        );

        let harvest = harvest(&ini, &ToggleState::default());
        let ranges = harvest.draws_for("ResourceBodyIB").expect("ranges");
        assert!(ranges
            .iter()
            .any(|range| range.count == 25884 && range.start == 973287));
    }

    #[test]
    fn collects_draw_ranges_from_active_drawindexed_lines() {
        let state = state(&[("apron", "0"), ("leotard", "1")]);
        let ini = lines(
            "[TextureOverrideTop]\r\n\
             hash = abc\r\n\
             ib = ResourceTopIB\r\n\
             if $apron == 0\r\n\
             \tdrawindexed = 7173, 0, 0\r\n\
             endif\r\n\
             if $leotard == 0\r\n\
             \tdrawindexed = 42390, 57801, 0\r\n\
             endif\r\n",
        );

        let harvest = harvest(&ini, &state);
        assert_eq!(
            harvest.draws_for("ResourceTopIB"),
            Some(&[range(7173, 0)][..])
        );
    }

    #[test]
    fn an_ib_whose_draws_are_all_disabled_has_no_ranges() {
        let state = state(&[("apron", "0")]);
        let ini = lines(
            "[TextureOverrideTop]\r\n\
             hash = abc\r\n\
             ib = ResourceTopIB\r\n\
             if $apron == 1\r\n\
             \tdrawindexed = 7173, 0, 0\r\n\
             endif\r\n",
        );

        let harvest = harvest(&ini, &state);
        // Explicitly empty, not `None`: `None` means "no drawindexed at all,
        // draw the whole buffer", which would show the disabled variant.
        assert_eq!(harvest.draws_for("ResourceTopIB"), Some(&[][..]));
    }

    #[test]
    fn reads_resource_names_with_transfer_prefixes() {
        assert_eq!(
            resource_name("ref ResourceBody.Base"),
            Some("resourcebody.base".to_string())
        );
        assert_eq!(
            resource_name("copy ResourceTop.Position"),
            Some("resourcetop.position".to_string())
        );
        assert_eq!(
            resource_name("ResourcePlain"),
            Some("resourceplain".to_string())
        );
        assert_eq!(resource_name("null"), None);
    }

    #[test]
    fn reads_saved_values_from_the_user_config_namespace() {
        let dir = std::env::temp_dir().join(format!("zzmm-toggle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        std::fs::write(
            dir.join("d3dx_user.ini"),
            "; AUTOMATICALLY GENERATED FILE\r\n[Constants]\r\n\
             $\\mods\\ellen swimsuit\\mod.ini\\swapkey = 2\r\n\
             $\\mods\\other mod\\mod.ini\\swapkey = 0\r\n",
        )
        .expect("write user config");

        let state = read_toggle_state(&[dir.clone()], "Ellen Swimsuit");
        assert!(state.from_user_config);
        assert_eq!(state.value("swapkey"), Some("2"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracks_bound_resources_and_clears_null_index_slots() {
        let ini = lines(
            "[TextureOverrideBody]\r\n\
             vb0 = ResourceBodyPosition\r\n\
             ib = ResourceBodyAIB\r\n\
             drawindexed = 3, 0, 0\r\n\
             ib = null\r\n\
             drawindexed = 3, 0, 0\r\n\
             ib = ResourceBodyBIB\r\n\
             drawindexed = 3, 0, 0\r\n",
        );

        let harvest = harvest(&ini, &ToggleState::default());

        // A resource on any binding line is "bound"; a declared-only resource
        // would be absent.
        assert!(harvest.bound_positions.contains("resourcebodyposition"));
        assert!(harvest.bound_indices.contains("resourcebodyaib"));
        assert!(harvest.bound_indices.contains("resourcebodybib"));
        assert!(harvest.cleared.contains("ib"));

        // The draw between `ib = null` and the next binding belongs to no IB,
        // so it must not clone BodyAIB's ranges.
        assert_eq!(
            harvest.draws_for("ResourceBodyAIB"),
            Some(&[range(3, 0)][..])
        );
        assert_eq!(
            harvest.draws_for("ResourceBodyBIB"),
            Some(&[range(3, 0)][..])
        );
    }

    #[test]
    fn omits_only_commented_draws_whose_hash_is_skipped() {
        // S11 shape: the replacement draws for B and C are commented out and
        // the shared hash sets `handling = skip`, so the game draws only A.
        let skipped = lines(
            "[TextureOverrideBodyIB]\r\n\
             hash = abc\r\n\
             handling = skip\r\n\
             \r\n\
             [TextureOverrideBodyA]\r\n\
             hash = abc\r\n\
             ib = ResourceBodyAIB\r\n\
             drawindexed = 3, 0, 0\r\n\
             \r\n\
             [TextureOverrideBodyB]\r\n\
             hash = abc\r\n\
             ib = ResourceBodyBIB\r\n\
             ; drawindexed = 9123, 0, 0\r\n",
        );
        let skipped_harvest = harvest(&skipped, &ToggleState::default());
        assert!(skipped_harvest.omits_index_draw("ResourceBodyBIB"));
        // A live draw is never an omission, commented or not.
        assert!(!skipped_harvest.omits_index_draw("ResourceBodyAIB"));
        assert!(skipped_harvest
            .disabled_draw_sections
            .contains_key("resourcebodybib"));

        // `#` is a comment prefix too; it must disable the draw the same way.
        let hash_comment = lines(
            "[TextureOverrideBodyIB]\r\n\
             hash = abc\r\n\
             handling = skip\r\n\
             \r\n\
             [TextureOverrideBodyB]\r\n\
             hash = abc\r\n\
             ib = ResourceBodyBIB\r\n\
             # drawindexed = 9123, 0, 0\r\n",
        );
        let hash_comment_harvest = harvest(&hash_comment, &ToggleState::default());
        assert!(hash_comment_harvest.omits_index_draw("ResourceBodyBIB"));

        // Without `handling = skip` the game still submits the draw, so a
        // commented line keeps the whole-buffer fallback.
        let not_skipped = lines(
            "[TextureOverrideBodyIB]\r\n\
             hash = abc\r\n\
             \r\n\
             [TextureOverrideBodyB]\r\n\
             hash = abc\r\n\
             ib = ResourceBodyBIB\r\n\
             ; drawindexed = 9123, 0, 0\r\n",
        );
        let not_skipped_harvest = harvest(&not_skipped, &ToggleState::default());
        assert!(!not_skipped_harvest.omits_index_draw("ResourceBodyBIB"));

        // `drawindexed = auto` redraws the whole bound buffer, so the group
        // does submit the part even though its hash is skipped.
        let auto = lines(
            "[TextureOverrideBodyIB]\r\n\
             hash = abc\r\n\
             handling = skip\r\n\
             drawindexed = auto\r\n\
             \r\n\
             [TextureOverrideBodyB]\r\n\
             hash = abc\r\n\
             ib = ResourceBodyBIB\r\n\
             ; drawindexed = 9123, 0, 0\r\n",
        );
        let auto_harvest = harvest(&auto, &ToggleState::default());
        assert!(!auto_harvest.omits_index_draw("ResourceBodyBIB"));

        // A skipped hash with no commented draw at all fails open.
        let no_comment = lines(
            "[TextureOverrideBodyIB]\r\n\
             hash = abc\r\n\
             handling = skip\r\n\
             \r\n\
             [TextureOverrideBodyB]\r\n\
             hash = abc\r\n\
             ib = ResourceBodyBIB\r\n",
        );
        let no_comment_harvest = harvest(&no_comment, &ToggleState::default());
        assert!(!no_comment_harvest.omits_index_draw("ResourceBodyBIB"));

        // A `handling = skip` inside a branch the saved state does not take
        // leaves the original draw in place, so the part is kept.
        let conditional = lines(
            "[TextureOverrideBodyIB]\r\n\
             hash = abc\r\n\
             if $hide == 1\r\n\
             handling = skip\r\n\
             endif\r\n\
             \r\n\
             [TextureOverrideBodyB]\r\n\
             hash = abc\r\n\
             ib = ResourceBodyBIB\r\n\
             ; drawindexed = 9123, 0, 0\r\n",
        );
        let state = state(&[("hide", "0")]);
        let conditional_harvest = harvest(&conditional, &state);
        assert!(!conditional_harvest.omits_index_draw("ResourceBodyBIB"));
    }

    #[test]
    fn matches_bound_aliases_by_component_core() {
        let bound: HashSet<String> = ["resourceoutfitvb".to_string()].into_iter().collect();
        assert!(binds_like(&bound, "ResourceOutfitPosition"));
        assert!(!binds_like(&bound, "ResourceHairPosition"));

        // An unparseable alias fails open so no part is ever lost.
        let unparseable: HashSet<String> =
            ["resourceposition981".to_string()].into_iter().collect();
        assert!(binds_like(&unparseable, "Resource981"));
    }
}
