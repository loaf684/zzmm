use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::ini_parser::commands::collect_ini_files;
use crate::ini_parser::{parser, text};
use crate::mesh_loader::decode;
use crate::mesh_loader::model::{MeshPart, MeshPreview, PreviewToggle, PreviewToggleValue};
use crate::mesh_loader::resources::{self, IniResource};
use crate::mesh_loader::variants::{self, Harvest, ToggleState};
use crate::mesh_loader::{encode_f32, encode_u32};
use crate::mods::{actions, metadata, scanner};
use crate::state::AppState;
use crate::texture;

const MAX_RESOURCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PARTS: usize = 24;
const MAX_INIS: usize = 16;

/// Files the loader/manager leaves behind: its own backups (`DISABLED_*`),
/// hand-made copies (`old seed.ini`, `backup ...`), and editor temp files.
/// Merging them adds a second, stale copy of every resource, which stacks
/// duplicate geometry and points textures at removed files.
fn is_ignored_ini_name(name: &str) -> bool {
    let name = name.trim();
    name.starts_with("disabled")
        || name.starts_with("old ")
        || name.starts_with("old_")
        || name.starts_with("old-")
        || name.starts_with("backup")
        || name.starts_with('~')
        || name.ends_with(".bak")
        || name.contains(" - copy")
        || name.contains("_backup")
}

fn mods_dir(state: &AppState) -> AppResult<PathBuf> {
    let dir = state
        .settings
        .read()
        .map_err(|_| AppError::StateLock)?
        .mods_dir
        .clone();

    dir.map(PathBuf::from).ok_or(AppError::NoModsDir)
}

#[tauri::command]
pub async fn load_mod_preview(
    state: State<'_, AppState>,
    folder_name: String,
    toggles: Option<Vec<PreviewToggleValue>>,
) -> AppResult<MeshPreview> {
    let mods = mods_dir(&state)?;

    tauri::async_runtime::spawn_blocking(move || build_preview(&mods, &folder_name, toggles))
        .await
        .map_err(|error| AppError::Task(error.to_string()))?
}

/// A parsed INI: its path relative to the mod root, its absolute path, and the
/// lines that feed both the resource scan and the toggle harvest.
type ParsedIni = (String, PathBuf, Vec<parser::IniLine>);

fn build_preview(
    mods: &Path,
    folder_name: &str,
    overrides: Option<Vec<PreviewToggleValue>>,
) -> AppResult<MeshPreview> {
    let root = actions::mod_dir(mods, folder_name)?;
    let meta = metadata::read(&root);
    let (_, base_name) = scanner::split_disabled(folder_name);
    let mod_name = meta
        .as_ref()
        .and_then(|meta| meta.name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| base_name.clone());

    let mut warnings = Vec::new();
    let mut buffers: Vec<IniResource> = Vec::new();
    let mut textures: Vec<IniResource> = Vec::new();
    let mut ini_used: Option<String> = None;

    // Read each INI once: resource sections and the `if`/`endif` command lists
    // that choose between variants come from the same lines.
    let mut parsed: Vec<ParsedIni> = Vec::new();
    for (relative, path, _size) in collect_ini_files(&root)
        .into_iter()
        // `DISABLED_*` INIs are the manager's own backups: reading them would
        // add a second copy of every resource, pointing at stale paths, and
        // stack duplicate geometry over the live variant.
        .filter(|(relative, _, _)| {
            let name = relative
                .rsplit('/')
                .next()
                .unwrap_or(relative)
                .to_ascii_lowercase();
            !is_ignored_ini_name(&name)
        })
        .take(MAX_INIS)
    {
        let Ok((content, _encoding)) = text::read(&path) else {
            continue;
        };
        parsed.push((relative, path, parser::split_lines(&content)));
    }

    // Toggle variables: INI defaults first, then whatever 3DMigoto saved for
    // this mod in `d3dx_user.ini` next to the loader.
    let mut toggle_state = ToggleState::default();
    for (_, _, lines) in &parsed {
        variants::collect_defaults(lines, &mut toggle_state);
    }

    let mut toggle_dirs = vec![root.clone(), mods.to_path_buf()];
    if let Some(parent) = mods.parent() {
        toggle_dirs.push(parent.to_path_buf());
        if let Some(grandparent) = parent.parent() {
            toggle_dirs.push(grandparent.to_path_buf());
        }
    }
    toggle_dirs.sort();
    toggle_dirs.dedup();

    let saved = variants::read_toggle_state(&toggle_dirs, &base_name);
    toggle_state.absorb(&saved);

    // System values 3DMigoto supplies at runtime. `DRAW_TYPE == 1` is the
    // normal colour pass; most mods bind their real vertex buffer only inside
    // that branch, so treating it as false (the old default) made the preview
    // fall back to guessed sibling buffers. `$active` marks the equipped mod;
    // a value saved by the user still wins because it was absorbed above.
    toggle_state.set_if_absent("draw_type", "1");
    toggle_state.set_if_absent("active", "1");

    // The viewer's explicit toggle choices. Only variables the mod actually
    // gates a binding on are accepted, so a stale control cannot perturb the
    // preview. Applied once before `[Present]` (so a selector override
    // propagates to its derived guards) and again after it (so an override of a
    // derived guard itself wins).
    let gate_variables: BTreeSet<String> = parsed
        .iter()
        .flat_map(|(_, _, lines)| variants::condition_variables(lines))
        .collect();
    let overrides = overrides.unwrap_or_default();
    apply_overrides(&mut toggle_state, &overrides, &gate_variables);

    // `[Present]` derives the draw toggles from the user's menu selection each
    // frame; replay it so the preview draws the outfit the game would.
    for (_, _, lines) in &parsed {
        variants::apply_present_assignments(lines, &mut toggle_state);
    }

    apply_overrides(&mut toggle_state, &overrides, &gate_variables);

    let mut harvest = Harvest::default();

    for (relative, path, lines) in &parsed {
        let found = resources::scan_lines(lines, Some(path.as_path()));

        // Texture-only INIs are common (`0.ini`, `seed/face.ini`): they hold
        // the diffuse resources and `this =` overrides but no buffers. Only
        // the mesh layout needs a buffer, never the colours.
        textures.extend(found.textures);
        harvest.absorb(variants::harvest(lines, &toggle_state));

        if !found.buffers.is_empty() {
            if ini_used.is_none() {
                ini_used = Some(relative.clone());
            }
            buffers.extend(found.buffers);
        }
    }

    let mut used_default_layout = buffers.iter().any(|buffer| buffer.stride.is_none());

    if buffers.is_empty() {
        warnings.push(
            "No [Resource*] buffers were found in the INI; scanning the mod folder for .buf files."
                .to_string(),
        );
        used_default_layout = true;

        let fallback = resources::scan_folder(&root);
        if fallback.buffers.is_empty() {
            return Err(AppError::Preview(
                "No .buf mesh buffers were found in this mod. Texture-only mods have no mesh to preview."
                    .to_string(),
            ));
        }

        buffers = fallback.buffers;
        textures = fallback.textures;
    }

    // Shader-dump mods keep their `.dds` files beside the buffers without
    // declaring a `[Resource…]` for each one (Miyabi), and some only declare
    // the hair maps (Nekomata). Index the folder too; strict matching still
    // decides whether a texture may paint a given component.
    for texture in resources::scan_folder(&root).textures {
        // The same file can be declared by an INI (relative to the INI folder)
        // and found by the folder scan (relative to its own directory). Compare
        // resolved paths so the colour map is not listed twice, which would
        // defeat the single-colour-map fallback.
        let duplicate = textures.iter().any(|existing| {
            match (resolve_file(&root, existing), resolve_file(&root, &texture)) {
                (Some(existing), Some(found)) => existing == found,
                _ => {
                    existing
                        .filename
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        == texture
                            .filename
                            .as_deref()
                            .unwrap_or_default()
                            .to_ascii_lowercase()
                        && existing.base_dir == texture.base_dir
                }
            }
        });
        if !duplicate {
            textures.push(texture);
        }
    }

    // Several INIs (a merged root file plus per-part files) can declare the
    // same buffer. Keeping one copy per resolved path prevents the merged
    // 16-file limit from rendering the same geometry twice.
    dedupe_resources_by_path(&root, &mut buffers);

    let position_buffers: Vec<IniResource> = buffers
        .iter()
        .filter(|buffer| buffer.is_position())
        .cloned()
        .collect();

    if position_buffers.is_empty() {
        return Err(AppError::Preview(
            "The INI resources contain no position buffer for this mod.".to_string(),
        ));
    }

    let mut parts = Vec::new();
    let mut hidden_parts = 0usize;
    let mut orphan_parts = 0usize;
    let mut culled_tris = 0usize;
    // Only warn about a missing colour map when the mod actually ships one for
    // something else; a prop-only mod legitimately has no diffuse at all.
    let ships_diffuse = textures
        .iter()
        .any(|texture| is_diffuse(&texture.lower_stem()));
    let declared_positions = position_buffers.len();

    // Colour maps shipped by other installed mods, used only as a last resort
    // for parts this mod leaves to the game's original texture. Never touches
    // this mod's own files.
    let library = TextureLibrary::build(mods, &root);

    // Variant copies (e.g. `AriaKFCTopPositionFlat` beside `AriaKFCTopPosition`)
    // decode the same component; drawing both stacks identical geometry. Keep
    // one position per component key, ranked by what the toggle state draws:
    // the variant the game binds, then one from the mod's root INI, then any
    // unbound copy. A variant the saved state excludes ranks last so a flat or
    // alternate copy never wins over the real part.
    let rank = |resource: &IniResource| {
        let key = resource.section.to_ascii_lowercase();
        let tier = if harvest.active.contains(&key) {
            0
        } else if harvest.excluded.contains(&key) {
            3
        } else if harvest.inactive.contains(&key) {
            2
        } else {
            1
        };
        let in_root = resource.base_dir.as_deref() == Some(root.as_path());
        (tier, !in_root)
    };

    let mut positions: Vec<IniResource> = Vec::new();
    for position in position_buffers {
        // A part whose buffers only sit inside an `if` branch that the toggle
        // state (INI defaults plus the saved `d3dx_user.ini` values) turns off
        // is a variant the game is not drawing: drop it before it can displace
        // the active variant.
        if harvest.hides(&position.section) {
            hidden_parts += 1;
            continue;
        }

        // A position that is only declared as a resource but never bound as
        // `vb0` is a disabled/commented-out part (Pulchra Body2). Only prune
        // when the mod has a binding table and another position exists, so a
        // single alias-named buffer is never lost.
        if declared_positions > 1
            && harvest.has_position_bindings()
            && !harvest.binds_position(&position.section)
        {
            orphan_parts += 1;
            continue;
        }

        let key = position.part_key();
        if key.is_empty() {
            positions.push(position);
            continue;
        }
        match positions.iter().position(|kept| kept.part_key() == key) {
            Some(existing) => {
                if rank(&position) < rank(&positions[existing]) {
                    positions[existing] = position;
                }
            }
            None => positions.push(position),
        }
    }

    for position in positions.iter().take(MAX_PARTS) {
        let key = position.part_key();
        let label = part_label(position);

        let Some(path) = resolve_file(&root, position) else {
            warnings.push(format!(
                "{label}: {} was not found on disk.",
                position.filename.clone().unwrap_or_default()
            ));
            continue;
        };

        let bytes = match read_limited(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                warnings.push(format!("{label}: {error}"));
                continue;
            }
        };

        let Some((stride, vertex_count)) =
            decode::position_layout(&bytes, position.stride, &mut warnings)
        else {
            warnings.push(format!(
                "{label}: no vertex stride fits {} bytes; skipped.",
                bytes.len()
            ));
            continue;
        };

        if position.stride.is_none() {
            used_default_layout = true;
        }

        let vertices = decode::read_positions(&bytes, stride, vertex_count);

        // UVs are only accepted when they line up with the position count.
        let mut source_files = vec![file_label(position)];
        let mut uvs: Option<Vec<[f32; 2]>> = None;

        if let Some(texcoord) = find_sibling(&buffers, &key, IniResource::is_texcoord, &harvest) {
            match resolve_file(&root, &texcoord) {
                Some(uv_path) => match read_limited(&uv_path) {
                    Ok(uv_bytes) => {
                        match decode::read_uvs(&uv_bytes, texcoord.stride, vertex_count) {
                            Some((_, decoded)) => {
                                uvs = Some(decoded);
                                source_files.push(file_label(&texcoord));
                            }
                            None => warnings.push(format!(
                                "{label}: texcoord buffer does not match the vertex count; rendering without UVs."
                            )),
                        }
                    }
                    Err(error) => warnings.push(format!(
                        "{label}: could not read {}: {error}",
                        texcoord.filename.clone().unwrap_or_default()
                    )),
                },
                None => warnings.push(format!(
                    "{label}: texcoord {} was not found on disk.",
                    texcoord.filename.clone().unwrap_or_default()
                )),
            }
        }

        // A component can be split over several index buffers. Sections that
        // share a `match_first_index` are alternative resolutions of the same
        // slice; distinct values are consecutive slices (lower body, upper
        // body, accessories) and every active slice belongs in the preview.
        let plans = select_index_plan(&root, &buffers, &key, &position.section, &harvest);
        let mut pieces: Vec<RenderPiece> = Vec::new();

        if plans.is_empty() {
            let (indices, index_resource) = resolve_indices(
                &root,
                &buffers,
                &key,
                vertex_count,
                &harvest,
                &mut warnings,
                &mut used_default_layout,
                &label,
            );
            if let Some(indices) = indices {
                pieces.push(RenderPiece {
                    label: label.clone(),
                    indices,
                    index_resource,
                    texture: None,
                });
            }
        } else {
            let plan_count = plans.len();
            for plan in plans {
                let Some(index_path) = resolve_file(&root, &plan.resource) else {
                    warnings.push(format!(
                        "{label}: index buffer {} was not found on disk.",
                        plan.resource.filename.clone().unwrap_or_default()
                    ));
                    continue;
                };
                let Ok(index_bytes) = read_limited(&index_path) else {
                    warnings.push(format!(
                        "{label}: could not read the index buffer {}.",
                        plan.resource.filename.clone().unwrap_or_default()
                    ));
                    continue;
                };
                if plan.resource.format.is_none() {
                    used_default_layout = true;
                }
                let Some(indices) = decode::decode_indices(
                    &index_bytes,
                    vertex_count,
                    plan.resource.format.as_deref(),
                    &mut warnings,
                ) else {
                    warnings.push(format!(
                        "{label}: index buffer {} does not fit the mesh; skipped.",
                        plan.resource.filename.clone().unwrap_or_default()
                    ));
                    continue;
                };

                let base_label = if plan_count > 1 {
                    part_label(&plan.resource)
                } else {
                    label.clone()
                };

                if let Some(ranges) = &plan.ranges {
                    let total: usize = ranges.iter().map(|range| range.count as usize).sum();
                    if total == 0 {
                        // The section has explicit `drawindexed` lines but every
                        // one sits in a branch the saved toggle state does not
                        // take. The game submits no geometry for this IB, so it
                        // must not fall back to drawing the whole buffer.
                        continue;
                    }
                    let fits = ranges.iter().all(|range| {
                        (range.start as usize).saturating_add(range.count as usize) <= indices.len()
                    });
                    if total > 0 && !fits {
                        warnings.push(format!(
                            "{label}: ignored draw ranges that do not fit the index buffer."
                        ));
                    } else if total > 0 {
                        // Group the sub-draws by the colour map bound at each
                        // draw. A section that swaps the diffuse mid-list (a
                        // merged hair + cape) then yields one piece per
                        // material instead of painting every range with the
                        // first draw's texture.
                        let mut groups: Vec<(Option<String>, Vec<u32>)> = Vec::new();
                        let mut kept = 0usize;
                        for range in ranges {
                            let from = (range.start as usize).min(indices.len());
                            let to = (from + range.count as usize).min(indices.len());
                            kept += to - from;
                            let slice = &indices[from..to];
                            match groups
                                .iter_mut()
                                .find(|(texture, _)| texture == &range.texture)
                            {
                                Some((_, buffer)) => buffer.extend_from_slice(slice),
                                None => groups.push((range.texture.clone(), slice.to_vec())),
                            }
                        }
                        if kept < indices.len() {
                            culled_tris += (indices.len() - kept) / 3;
                        }

                        let multiple = groups.len() > 1;
                        for (texture, group_indices) in groups {
                            let piece_label = if multiple {
                                let tag = match texture.as_deref() {
                                    Some(resource) => short_texture_tag(resource),
                                    None => "original".to_string(),
                                };
                                format!("{base_label} [{tag}]")
                            } else {
                                base_label.clone()
                            };
                            pieces.push(RenderPiece {
                                label: piece_label,
                                indices: group_indices,
                                index_resource: Some(plan.resource.clone()),
                                texture,
                            });
                        }
                        continue;
                    }
                }

                pieces.push(RenderPiece {
                    label: base_label,
                    indices,
                    index_resource: Some(plan.resource),
                    texture: None,
                });
            }
        }

        for piece in pieces {
            if parts.len() >= MAX_PARTS {
                break;
            }

            let indices = piece.indices;
            let normals = decode::compute_normals(&vertices, &indices);

            let piece_scope = piece
                .index_resource
                .as_ref()
                .and_then(|resource| resource.base_dir.as_deref());

            let mut texture_match = if let Some(name) = piece.texture.as_deref() {
                // This sub-draw's own binding is authoritative: it says exactly
                // which colour map the game paints this range with, even when a
                // sibling range in the same IB uses a different one.
                load_bound_texture(
                    &root,
                    &textures,
                    name,
                    piece_scope,
                    &mut warnings,
                    &piece.label,
                )
            } else {
                let preferred = piece
                    .index_resource
                    .as_ref()
                    .and_then(|resource| harvest.texture_for_index(&resource.section))
                    .or_else(|| harvest.texture_for(&position.section));
                load_texture(
                    &root,
                    &textures,
                    &key,
                    preferred,
                    piece.index_resource.as_ref(),
                    Some(&harvest),
                    &mut warnings,
                )
            };

            // A part with no colour map in its own mod can still be painted when
            // another installed mod ships the same character's atlas (a skin mod
            // that leaves the body to the game, while a sibling mod provides it).
            if texture_match.url.is_none() && texture_match.error.is_none() {
                let mut characters: Vec<String> = Vec::new();
                for candidate in [&key, &mod_name] {
                    if let Some(character) = character_token(candidate) {
                        if !characters.contains(&character) {
                            characters.push(character);
                        }
                    }
                }
                for character in characters {
                    if let Some(path) = library.find(&key, &character, &root) {
                        let source = path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_default();
                        warnings.push(format!(
                            "{}: no diffuse is shipped in this mod; using {source} from the mod library for this character.",
                            piece.label
                        ));
                        texture_match = finish_texture_path(&path, &mut warnings);
                        break;
                    }
                }
            }

            // A component with UVs and no colour map at all is usually a mod
            // that expects the game's original texture. Say so explicitly
            // instead of silently painting it with the neutral clay colour.
            if texture_match.url.is_none()
                && texture_match.error.is_none()
                && piece.texture.is_none()
                && ships_diffuse
                && uvs.is_some()
            {
                warnings.push(format!(
                    "{label}: no diffuse texture is shipped for this part; rendered with the neutral material."
                ));
            }

            let mut files = source_files.clone();
            if let Some(resource) = &piece.index_resource {
                files.push(file_label(resource));
            }
            files.sort();
            files.dedup();

            parts.push(MeshPart {
                name: piece.label,
                vertex_count: vertex_count as u32,
                triangle_count: (indices.len() / 3) as u32,
                positions: encode_f32(
                    &vertices
                        .iter()
                        .flat_map(|vertex| vertex.iter().copied())
                        .collect::<Vec<_>>(),
                ),
                normals: encode_f32(&normals),
                uvs: uvs
                    .as_ref()
                    .map(|uvs| {
                        encode_f32(
                            &uvs.iter()
                                .flat_map(|uv| uv.iter().copied())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .unwrap_or_default(),
                indices: encode_u32(&indices),
                texture: texture_match.url,
                texture_error: texture_match.error,
                material: texture_match.material,
                has_alpha: texture_match.has_alpha,
                source_files: files,
            });
        }
    }

    if hidden_parts > 0 {
        warnings.push(format!(
            "{hidden_parts} alternative part(s) hidden to match the in-game toggle state."
        ));
    }

    if orphan_parts > 0 {
        warnings.push(format!(
            "{orphan_parts} part(s) declared in the INI are never bound by a draw; skipped."
        ));
    }

    if culled_tris > 0 {
        warnings.push(format!(
            "Skipped {culled_tris} triangles from variants the saved toggle state does not draw."
        ));
    }

    if parts.is_empty() {
        warnings.push("No part could be decoded from the available buffers.".to_string());
        return Err(AppError::Preview(if warnings.is_empty() {
            "No renderable mesh was found in this mod.".to_string()
        } else {
            warnings.join(" ")
        }));
    }

    let toggles = supported_toggles(&parsed, &toggle_state, &harvest);

    Ok(MeshPreview {
        mod_folder: folder_name.to_string(),
        mod_name,
        ini_file: ini_used,
        parts,
        warnings,
        used_default_layout,
        toggles_applied: hidden_parts > 0 || culled_tris > 0 || saved.from_user_config,
        toggles,
    })
}

/// Apply the viewer's toggle overrides, ignoring names the mod never gates on.
fn apply_overrides(
    state: &mut ToggleState,
    overrides: &[PreviewToggleValue],
    allowed: &BTreeSet<String>,
) {
    for entry in overrides {
        let name = entry
            .name
            .trim()
            .trim_start_matches('$')
            .to_ascii_lowercase();
        if name.is_empty() || !allowed.contains(&name) {
            continue;
        }
        state.set(&name, entry.value.trim());
    }
}

/// Candidate toggle variables and the numeric values the INI gives them,
/// merged across every INI of the mod.
fn toggle_candidates(
    parsed: &[ParsedIni],
    state: &ToggleState,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut candidates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (_, _, lines) in parsed {
        for name in variants::condition_variables(lines) {
            let values = candidates.entry(name.clone()).or_default();
            values.extend(variants::candidate_values(lines, &name));
        }
    }
    for (name, values) in &mut candidates {
        if let Some(current) = state.value(name).and_then(variants::numeric_literal) {
            values.insert(current);
        }
        // 3DMigoto's boolean idiom writes only one branch (`if $x == 0 ... else
        // ...`), so a lone `0` or `1` is completed to the pair. A selector that
        // lists several values keeps exactly the listed domain.
        if values.contains("0") && !values.contains("1") {
            values.insert("1".to_string());
        }
        if values.contains("1") && !values.contains("0") {
            values.insert("0".to_string());
        }
    }
    candidates
}

fn harvest_all(parsed: &[ParsedIni], state: &ToggleState) -> Harvest {
    let mut merged = Harvest::default();
    for (_, _, lines) in parsed {
        merged.absorb(variants::harvest(lines, state));
    }
    merged
}

/// Variables whose value actually changes what the preview draws.
///
/// A candidate is a `$variable` used in an `if` / `elif` / `else if` condition
/// with at least two known numeric values. It is exposed only when at least one
/// alternative value produces a different harvest fingerprint than the state
/// the model is rendered with, so a control appears exactly when it changes the
/// mesh, its parts, or the colour map/draw ranges the game submits. Variables
/// that gate nothing on screen are never returned.
fn supported_toggles(
    parsed: &[ParsedIni],
    state: &ToggleState,
    harvest: &Harvest,
) -> Vec<PreviewToggle> {
    let baseline = variants::harvest_signature(harvest);
    let mut toggles = Vec::new();

    for (name, options) in toggle_candidates(parsed, state) {
        if options.len() < 2 {
            continue;
        }
        // Only a variable currently in a numeric state can be shown as a
        // discrete control; a text setting has no meaningful option list.
        let Some(current) = state.value(&name).and_then(variants::numeric_literal) else {
            continue;
        };

        let affects = options
            .iter()
            .filter(|option| **option != current)
            .any(|option| {
                let mut probe = state.clone();
                probe.set(&name, option);
                variants::harvest_signature(&harvest_all(parsed, &probe)) != baseline
            });
        if !affects {
            continue;
        }

        toggles.push(PreviewToggle {
            label: toggle_label(&name),
            name,
            value: current,
            options: options.into_iter().collect(),
        });
    }

    toggles
}

/// `show_hat` → `Show hat`, `swapkey` → `Swapkey`.
fn toggle_label(name: &str) -> String {
    let cleaned = name.trim_start_matches('$').replace('_', " ");
    let mut chars = cleaned.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn part_label(resource: &IniResource) -> String {
    let stem = resource.file_stem();
    let lower = stem.to_ascii_lowercase();

    for marker in [
        "position", "texcoord", "blend", "vertex", "indices", "index",
    ] {
        if let Some(found) = lower.find(marker) {
            let trimmed = stem[..found]
                .trim_matches(|character| character == '_' || character == '-' || character == '.');
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    if stem.is_empty() {
        "Mesh".to_string()
    } else {
        stem
    }
}

fn file_label(resource: &IniResource) -> String {
    resource
        .filename
        .clone()
        .unwrap_or_else(|| resource.section.clone())
}

/// Find a matching buffer by shared component key. Falls back to a loose
/// component-core match (`AriaKFCTopA` ← `AriaKFCTop`), then to the only
/// candidate, then to unnamed buffers.
fn find_sibling(
    buffers: &[IniResource],
    key: &str,
    matcher: fn(&IniResource) -> bool,
    harvest: &Harvest,
) -> Option<IniResource> {
    let mut candidates: Vec<&IniResource> =
        buffers.iter().filter(|buffer| matcher(buffer)).collect();

    // When several buffers share the component (`BodyAIB` / `BodyBIB`, a flat
    // copy beside the real mesh), the variant the toggle state draws must win
    // over whichever one happened to appear first in the INI.
    candidates.sort_by_key(|buffer| {
        !harvest
            .active
            .contains(&buffer.section.to_ascii_lowercase())
    });

    if !key.is_empty() {
        if let Some(found) = candidates.iter().find(|buffer| buffer.part_key() == key) {
            return Some((*found).clone());
        }

        // Index buffers usually carry a short qualifier the position buffer
        // lacks (`AriaKFCTopA.ib` for `AriaKFCTopPosition.buf`).
        let key_core = resources::component_core(key);
        let loose: Vec<(&IniResource, String)> = candidates
            .iter()
            .map(|buffer| (*buffer, resources::component_core(&buffer.part_key())))
            .filter(|(_, core)| resources::cores_match(core, &key_core))
            .collect();
        if let Some((best, _)) = loose
            .iter()
            .min_by_key(|(_, core)| core.len().abs_diff(key_core.len()))
        {
            let found = (**best).clone();
            return Some(found);
        }
    }

    if candidates.len() == 1 {
        return Some(candidates[0].clone());
    }

    candidates
        .iter()
        .find(|buffer| buffer.part_key().is_empty())
        .map(|buffer| (*buffer).clone())
}

struct RenderPiece {
    label: String,
    indices: Vec<u32>,
    index_resource: Option<IniResource>,
    /// Colour-map resource bound for this specific sub-draw range. `None` keeps
    /// the section-level matching for pieces that were not split by texture.
    texture: Option<String>,
}

struct IndexPlan {
    resource: IniResource,
    ranges: Option<Vec<variants::DrawRange>>,
}

/// Index buffers that belong to one position. Consecutive slices of a split
/// mesh have different `match_first_index` values and are all kept; buffers
/// with the same value are alternative resolutions of one draw, so only the
/// best-ranked one survives.
fn select_index_plan(
    root: &Path,
    buffers: &[IniResource],
    key: &str,
    position_section: &str,
    harvest: &Harvest,
) -> Vec<IndexPlan> {
    let position = buffers
        .iter()
        .find(|buffer| buffer.section.eq_ignore_ascii_case(position_section));
    let position_core = resources::component_core(position_section);

    let mut candidates: Vec<IniResource> = buffers
        .iter()
        .filter(|buffer| buffer.is_index())
        .filter(|buffer| !harvest.hides(&buffer.section))
        .filter(|buffer| !harvest.has_index_bindings() || harvest.binds_index(&buffer.section))
        .filter(|buffer| index_belongs(buffer, key, &position_core, harvest))
        .filter(|buffer| position.is_none_or(|own| same_variant(own, buffer)))
        .cloned()
        .collect();

    // An IB's own component name outranks an alias mapping from a section that
    // also draws a sibling mesh. Belle binds the legs IB beside the body VB, so
    // the alias table says "legs" while the file clearly names the body. When
    // at least one IB is named after this component, drop IBs that are named
    // after a *different* component; unnamed/hash IBs stay (fail open).
    let key_core = resources::component_core(key);
    let self_named = |buffer: &IniResource| {
        let core = resources::component_core(&buffer.part_key());
        !core.is_empty()
            && ((!key_core.is_empty() && resources::cores_match(&core, &key_core))
                || (!position_core.is_empty() && resources::cores_match(&core, &position_core)))
    };
    if candidates.iter().any(self_named) {
        candidates.retain(|buffer| {
            self_named(buffer) || resources::component_core(&buffer.part_key()).is_empty()
        });
    }

    candidates.sort_by_key(|buffer| index_rank(root, buffer, harvest, key, &position_core));

    let mut chosen: Vec<(u32, String, IniResource)> = Vec::new();
    for candidate in candidates {
        let first = harvest
            .index_first
            .get(&candidate.section.to_ascii_lowercase())
            .copied()
            .unwrap_or(0);
        let role = harvest
            .position_for_index(&candidate.section)
            .map(resources::component_core)
            .filter(|role| !role.is_empty())
            .unwrap_or_else(|| resources::component_core(&candidate.section));
        if chosen
            .iter()
            .any(|(own_first, own_role, _)| *own_first == first && *own_role == role)
        {
            continue;
        }
        chosen.push((first, role, candidate));
    }

    chosen.sort_by_key(|(first, _, _)| *first);
    chosen
        .into_iter()
        .map(|(_, _, resource)| {
            let ranges = harvest.draws_for(&resource.section).map(|ranges| {
                let mut unique: Vec<variants::DrawRange> = Vec::new();
                for range in ranges {
                    if !unique
                        .iter()
                        .any(|kept| kept.count == range.count && kept.start == range.start)
                    {
                        unique.push(range.clone());
                    }
                }
                unique
            });
            IndexPlan { resource, ranges }
        })
        .collect()
}

fn index_belongs(index: &IniResource, key: &str, position_core: &str, harvest: &Harvest) -> bool {
    if let Some(mapped) = harvest.position_for_index(&index.section) {
        let mapped_core = resources::component_core(mapped);
        // A mapped alias (`ResourceOutfitVB` → `outfit`) need not spell the
        // full component name; fall through to the IB's own name before
        // rejecting it.
        if !mapped_core.is_empty()
            && !position_core.is_empty()
            && resources::cores_match(&mapped_core, position_core)
        {
            return true;
        }
    }

    let index_key = index.part_key();
    if !key.is_empty() && index_key == *key {
        return true;
    }
    if key.is_empty() || index_key.is_empty() {
        return true;
    }

    resources::cores_match(
        &resources::component_core(&index_key),
        &resources::component_core(key),
    )
}

fn index_rank(
    root: &Path,
    resource: &IniResource,
    harvest: &Harvest,
    key: &str,
    position_core: &str,
) -> (u8, u8, u8, String) {
    let name = resource.section.to_ascii_lowercase();
    let index_core = resources::component_core(&resource.part_key());
    let key_core = resources::component_core(key);

    // An IB whose own name names this component is a better fit than one that
    // only happens to be bound through `vb0` in a section that draws several
    // meshes (Miyabi draws the 981 buffer under both VB resources).
    let named = (!key.is_empty() && resource.part_key() == *key)
        || (!key_core.is_empty()
            && !index_core.is_empty()
            && resources::cores_match(&index_core, &key_core))
        || (!position_core.is_empty()
            && !index_core.is_empty()
            && resources::cores_match(&index_core, position_core));
    let mapped = harvest
        .position_for_index(&resource.section)
        .map(resources::component_core)
        .is_some_and(|core| {
            !core.is_empty()
                && !position_core.is_empty()
                && resources::cores_match(&core, position_core)
        });
    let identity = if named {
        0
    } else if mapped {
        1
    } else {
        2
    };

    let tier = if harvest.active.contains(&name) {
        0
    } else if harvest.inactive.contains(&name) {
        2
    } else {
        1
    };
    let root_rank = if resource.base_dir.as_deref() == Some(root) {
        0
    } else {
        1
    };
    (identity, tier, root_rank, name)
}

/// Two buffers belong to the same mod variant when they live in the same
/// subdirectory (Grace/Rina ship several `.01`–`.04` copies side by side).
fn same_variant(left: &IniResource, right: &IniResource) -> bool {
    fn scope(resource: &IniResource) -> Option<String> {
        let name = resource.filename.as_deref()?.replace('\\', "/");
        let (dir, _) = name.rsplit_once('/')?;
        let dir = dir
            .trim_start_matches("./")
            .trim_matches('/')
            .to_ascii_lowercase();
        (!dir.is_empty()).then_some(dir)
    }

    match (scope(left), scope(right)) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

fn resolve_indices(
    root: &Path,
    buffers: &[IniResource],
    key: &str,
    vertex_count: usize,
    harvest: &Harvest,
    warnings: &mut Vec<String>,
    used_default_layout: &mut bool,
    label: &str,
) -> (Option<Vec<u32>>, Option<IniResource>) {
    let Some(index_resource) = find_sibling(buffers, key, IniResource::is_index, harvest) else {
        if vertex_count % 3 == 0 {
            warnings.push(format!(
                "{label}: no index buffer; assuming sequential triangles."
            ));
            *used_default_layout = true;
            return (Some(decode::sequential_indices(vertex_count)), None);
        }
        return (None, None);
    };

    let Some(path) = resolve_file(root, &index_resource) else {
        warnings.push(format!(
            "{label}: index buffer {} was not found on disk.",
            index_resource.filename.clone().unwrap_or_default()
        ));
        return (None, None);
    };

    let Ok(bytes) = read_limited(&path) else {
        warnings.push(format!(
            "{label}: could not read the index buffer {}.",
            index_resource.filename.clone().unwrap_or_default()
        ));
        return (None, None);
    };

    if index_resource.format.is_none() {
        *used_default_layout = true;
    }

    match decode::decode_indices(
        &bytes,
        vertex_count,
        index_resource.format.as_deref(),
        warnings,
    ) {
        Some(indices) => (Some(indices), Some(index_resource)),
        None => {
            warnings.push(format!(
                "{label}: index buffer does not fit the mesh; assuming sequential triangles."
            ));
            if vertex_count % 3 == 0 {
                (
                    Some(decode::sequential_indices(vertex_count)),
                    Some(index_resource),
                )
            } else {
                (None, None)
            }
        }
    }
}

/// One resolved colour map: the PNG data URL, any diagnostic, the source file,
/// and whether the texture uses alpha (for alpha-tested cutout rendering).
#[derive(Debug, Default)]
struct TextureMatch {
    url: Option<String>,
    error: Option<String>,
    material: Option<String>,
    has_alpha: bool,
}

fn load_texture(
    root: &Path,
    textures: &[IniResource],
    key: &str,
    preferred: Option<&str>,
    index: Option<&IniResource>,
    harvest: Option<&Harvest>,
    warnings: &mut Vec<String>,
) -> TextureMatch {
    let key_is_head = [
        "face", "head", "makeup", "eye", "iris", "mouth", "teeth", "brow",
    ]
    .iter()
    .any(|marker| key.contains(marker));

    let visible: Vec<&IniResource> = textures
        .iter()
        .filter(|texture| !harvest.is_some_and(|harvest| harvest.hides(&texture.section)))
        .filter(|texture| !is_ui_texture(texture))
        .filter(|texture| key_is_head || !is_head_texture(&texture.lower_stem()))
        .collect();

    if visible.is_empty() {
        return TextureMatch::default();
    }

    // The INI folder that submitted this draw, used to disambiguate resource
    // names that several sub-INIs redeclare with different files.
    let texture_scope = index.and_then(|index| index.base_dir.as_deref());

    let ib_core = index
        .map(|index| resources::component_core(&index.section))
        .unwrap_or_default();
    let source_confident = index.is_some_and(|index| {
        harvest
            .and_then(|harvest| {
                harvest
                    .index_texture_section
                    .get(&index.section.to_ascii_lowercase())
            })
            .is_some_and(|source| {
                !ib_core.is_empty()
                    && resources::cores_match(&resources::component_core(source), &ib_core)
            })
    });

    let preferred_texture = preferred
        .and_then(|preferred| named_resource(&visible, preferred, texture_scope))
        .filter(|chosen| !is_data_map(&chosen.lower_stem()));

    // 1. A binding made by the IB's own section is trustworthy even when the
    //    texture name does not contain the component (Yuzuha's Tenga skin).
    if source_confident {
        if let Some(chosen) = preferred_texture {
            return finish_texture(root, chosen, warnings);
        }
    }

    // 2. The IB's own identity is the strongest signal for split and
    //    shader-dump meshes (Body1B → Body1BDiffuse, 981c1a1e → 981c1a1e…DiffuseMap).
    if ib_core.len() >= 2 {
        if let Some(chosen) = visible.iter().copied().find(|texture| {
            let stem = texture.lower_stem();
            !is_data_map(&stem)
                && stem.starts_with(&ib_core)
                && resolve_file(root, texture).is_some()
        }) {
            return finish_texture(root, chosen, warnings);
        }
    }

    // 3. The explicit binding (diffuse/ps-t0/`this`) after the own-identity
    //    check, so a section that draws a sibling mesh cannot leak its colour
    //    map onto this one.
    if let Some(chosen) = preferred_texture {
        return finish_texture(root, chosen, warnings);
    }

    // 4. A texture override (`TextureOverride…Diffuse`) supplies the colour
    //    map for the component named in its section. This covers `this = …`
    //    texture swaps, which have no mesh binding at all.
    if let Some(harvest) = harvest {
        let mut cores: Vec<String> = Vec::new();
        if !ib_core.is_empty() {
            cores.push(ib_core.clone());
        }
        if !key.is_empty() {
            cores.push(resources::component_core(key));
        }

        let mut best: Option<(usize, &str)> = None;
        for (section, target) in &harvest.named_textures {
            let core = resources::component_core(section);
            for mesh in &cores {
                if resources::cores_match(&core, mesh) {
                    let common = common_prefix_len(&core, mesh);
                    if best.is_none_or(|(length, _)| common > length) {
                        best = Some((common, target.as_str()));
                    }
                }
            }
        }

        if let Some((_, target)) = best {
            if let Some(chosen) = named_resource(&visible, target, texture_scope) {
                if !is_data_map(&chosen.lower_stem()) {
                    return finish_texture(root, chosen, warnings);
                }
            }
        }
    }

    // 5. The full IB stem, then the resolved-link table.
    if let Some(index) = index {
        let stem = index.lower_stem();
        if !stem.is_empty() {
            if let Some(chosen) = visible.iter().copied().find(|texture| {
                let candidate = texture.lower_stem();
                !is_data_map(&candidate)
                    && candidate.starts_with(&stem)
                    && resolve_file(root, texture).is_some()
            }) {
                return finish_texture(root, chosen, warnings);
            }
        }

        if let Some(harvest) = harvest {
            if let Some((_, target)) = harvest.links.iter().find(|(mesh, _)| {
                resources::cores_match(&resources::component_core(mesh), &ib_core)
            }) {
                if let Some(chosen) = named_resource(&visible, target, texture_scope) {
                    if !is_data_map(&chosen.lower_stem()) {
                        return finish_texture(root, chosen, warnings);
                    }
                }
            }
        }
    }

    // 6. A colour map whose name mentions this component.
    let matching: Vec<&IniResource> = visible
        .iter()
        .copied()
        .filter(|texture| {
            !key.is_empty()
                && !is_data_map(&texture.lower_stem())
                && texture.lower_stem().contains(key)
        })
        .collect();

    let chosen = matching
        .iter()
        .copied()
        .find(|texture| is_diffuse(&texture.lower_stem()))
        .or_else(|| {
            matching
                .iter()
                .copied()
                .find(|texture| !is_data_map(&texture.lower_stem()))
        });

    if let Some(chosen) = chosen {
        return finish_texture(root, chosen, warnings);
    }

    // Nothing named this component. Leave it neutral: reusing the mod's only
    // colour map paints knives, glasses, and cloth with the skin atlas.
    TextureMatch::default()
}

fn is_ui_texture(texture: &IniResource) -> bool {
    let path = texture
        .filename
        .as_deref()
        .unwrap_or_default()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let stem = texture.lower_stem();

    path.contains("/toggles/")
        || path.contains("/toggle/")
        || ["renderimage", "credit", "preview", "menubg"]
            .iter()
            .any(|marker| stem.contains(marker))
}

fn common_prefix_len(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .count()
}

/// Face/makeup maps belong to the head. Painting a body component with one is
/// worse than leaving it neutral.
fn is_head_texture(stem: &str) -> bool {
    [
        "face", "makeup", "blush", "lips", "iris", "eyebrow", "eyelash", "teeth", "mouth",
    ]
    .iter()
    .any(|marker| stem.contains(marker))
}

fn is_data_map(stem: &str) -> bool {
    [
        "normal",
        "lightmap",
        "light",
        "glow",
        "materialmap",
        "mask",
        "specular",
        "roughness",
        "metalness",
        "metallic",
        "emissive",
        "heightmap",
        "opacity",
        "alpha",
        "shader",
        "wengine",
        "_fx",
        "null",
    ]
    .iter()
    .any(|marker| stem.contains(marker))
}

fn is_diffuse(stem: &str) -> bool {
    !is_data_map(stem)
        && (stem.contains("diffuse")
            || stem.contains("albedo")
            || stem.contains("basecolor")
            || stem.contains("base_color"))
}

/// Match a texture resource by its `[Resource…]` section name (cover names
/// such as `ResourceBody.Base`) or by file stem.
fn named_resource<'a>(
    candidates: &[&'a IniResource],
    name: &str,
    scope: Option<&Path>,
) -> Option<&'a IniResource> {
    // Large multi-INI mods (a root body INI plus per-model subfolders) reuse
    // generic resource names like `Resource_Body_Diffuse` with different files.
    // Prefer the definition that lives in the same folder as the draw, so the
    // body does not get painted with a sibling model's atlas. Fall back to any
    // match: texture-only INIs legitimately declare colours away from the mesh.
    if let Some(scope) = scope {
        if let Some(chosen) = candidates.iter().copied().find(|texture| {
            texture.filename.is_some()
                && (texture.section.eq_ignore_ascii_case(name) || texture.lower_stem() == name)
                && texture.base_dir.as_deref() == Some(scope)
        }) {
            return Some(chosen);
        }
    }
    candidates.iter().copied().find(|texture| {
        texture.filename.is_some()
            && (texture.section.eq_ignore_ascii_case(name) || texture.lower_stem() == name)
    })
}

fn finish_texture(root: &Path, chosen: &IniResource, warnings: &mut Vec<String>) -> TextureMatch {
    let material = chosen.filename.clone();

    let Some(path) = resolve_file(root, chosen) else {
        let name = chosen.filename.clone().unwrap_or_default();
        warnings.push(format!("Texture {name} was not found on disk."));
        return TextureMatch {
            error: Some(format!("{name} is missing.")),
            material,
            ..TextureMatch::default()
        };
    };

    match texture::load_preview_texture(&path) {
        Ok(preview) => TextureMatch {
            url: Some(preview.data_url),
            error: None,
            material,
            has_alpha: preview.has_alpha,
        },
        Err(error) => {
            warnings.push(format!(
                "Could not decode {}: {error}",
                chosen.filename.clone().unwrap_or_default()
            ));
            TextureMatch {
                error: Some(error.to_string()),
                material,
                ..TextureMatch::default()
            }
        }
    }
}

/// Load the colour map a specific sub-draw bound. When the resource has no file
/// on disk (a texture 3DMigoto captured from the running game) the part is left
/// neutral and the reason is reported instead of painting it with a sibling
/// section's atlas.
fn load_bound_texture(
    root: &Path,
    textures: &[IniResource],
    name: &str,
    scope: Option<&Path>,
    warnings: &mut Vec<String>,
    label: &str,
) -> TextureMatch {
    let candidates: Vec<&IniResource> = textures
        .iter()
        .filter(|texture| texture.filename.is_some())
        .collect();

    if let Some(chosen) = named_resource(&candidates, name, scope) {
        return finish_texture(root, chosen, warnings);
    }

    warnings.push(format!(
        "{label}: this draw uses a texture captured from the running game ({name}) that is not stored on disk; rendered with the neutral material."
    ));
    TextureMatch::default()
}

fn finish_texture_path(path: &Path, warnings: &mut Vec<String>) -> TextureMatch {
    let material = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string());

    match texture::load_preview_texture(path) {
        Ok(preview) => TextureMatch {
            url: Some(preview.data_url),
            error: None,
            material,
            has_alpha: preview.has_alpha,
        },
        Err(error) => {
            warnings.push(format!("Could not decode {}: {error}", path.display()));
            TextureMatch {
                error: Some(error.to_string()),
                material,
                ..TextureMatch::default()
            }
        }
    }
}

/// Component words that follow a character name in file, resource and mesh
/// names: `NekomataBodyPosition` → character `nekomata`, component `body`.
const COMPONENT_WORDS: &[&str] = &[
    "body", "legs", "leg", "hair", "face", "head", "coat", "gourd", "arm", "hand", "skin", "eye",
    "tail", "ear", "cloth", "weapon", "wing", "dress", "skirt", "shoe", "boot", "glove", "hat",
    "neck", "glass", "gun", "sword", "wea",
];

/// Split a name into the character it belongs to and the part it names. The
/// character is the leading alphabetic run before the first component word.
/// Names without a usable run (`A_Special_Day…`) yield `None`.
fn split_character_name(name: &str) -> Option<(String, String)> {
    let lower = name.to_ascii_lowercase();
    let run: String = lower
        .chars()
        .take_while(|character| character.is_ascii_alphabetic())
        .collect();
    if run.is_empty() {
        return None;
    }

    let mut cut = run.len();
    let mut component = String::new();
    for word in COMPONENT_WORDS {
        if let Some(position) = run.find(word) {
            if position > 0 && position < cut {
                cut = position;
                component = (*word).to_string();
            }
        }
    }

    let mut character = run[..cut].to_string();
    if let Some(stripped) = character.strip_suffix("mod") {
        if stripped.len() >= 4 {
            character = stripped.to_string();
        }
    }
    if character.len() < 4 {
        return None;
    }

    if component.is_empty() {
        // No known part word: the whole remainder is the component (usually
        // empty for a bare `BodyPosition` style name).
        component = run[cut..].to_string();
    }

    Some((character, component))
}

fn character_token(name: &str) -> Option<String> {
    split_character_name(name).map(|(character, _)| character)
}

/// Parts drawn on the character's body UV sheet. They may borrow another
/// mod's body atlas for the same character. Accessories (coat, gourd, glasses,
/// hair) have their own island and must never be painted with a body sheet.
fn is_body_group(component: Option<&str>) -> bool {
    matches!(
        component,
        Some(
            "body"
                | "legs"
                | "leg"
                | "arm"
                | "arms"
                | "hand"
                | "hands"
                | "skin"
                | "torso"
                | "waist"
                | "foot"
                | "feet"
        )
    )
}

/// A short, human readable tag for a bound colour resource, used to label the
/// per-material pieces a split section produces.
fn short_texture_tag(resource: &str) -> String {
    let mut tag = resource.to_ascii_lowercase();
    for prefix in ["textureoverride", "resource"] {
        if let Some(rest) = tag.strip_prefix(prefix) {
            tag = rest.to_string();
            break;
        }
    }
    tag = tag
        .trim_matches(|character: char| character == '_' || character == '-' || character == '.')
        .to_string();
    for marker in [
        "adiffuse",
        "bdiffuse",
        "cdiffuse",
        "diffuse",
        "albedo",
        "basecolor",
    ] {
        if let Some(position) = tag.find(marker) {
            tag.truncate(position);
        }
    }
    let tag = tag
        .trim_matches(|character: char| character == '_' || character == '-' || character == '.');
    if tag.is_empty() {
        "texture".to_string()
    } else {
        tag.chars().take(20).collect()
    }
}

const MAX_LIBRARY_ENTRIES: usize = 4000;

/// Diffuse maps other installed mods ship, indexed by the character parsed out
/// of the file name. Used only as a last resort for a part whose own mod
/// deliberately leaves the colour to the game's original texture. Files under
/// the current mod are never considered.
#[derive(Default)]
struct TextureLibrary {
    entries: Vec<TextureEntry>,
}

struct TextureEntry {
    path: PathBuf,
    character: String,
    component: Option<String>,
    is_head: bool,
}

impl TextureLibrary {
    fn build(mods: &Path, root: &Path) -> Self {
        let mut library = TextureLibrary::default();
        let Ok(entries) = fs::read_dir(mods) else {
            return library;
        };

        for entry in entries.flatten() {
            if library.entries.len() >= MAX_LIBRARY_ENTRIES {
                break;
            }
            let dir = entry.path();
            if !dir.is_dir() || dir.as_path() == root {
                continue;
            }

            let mut files = Vec::new();
            collect_diffuse_files(&dir, 0, &mut files);
            for file in files {
                let stem = file
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default();
                if !is_diffuse(&stem) {
                    continue;
                }
                let Some((character, component)) = split_character_name(&stem) else {
                    continue;
                };
                library.entries.push(TextureEntry {
                    path: file,
                    character,
                    component: (!component.is_empty()).then_some(component),
                    is_head: is_head_texture(&stem),
                });
            }
        }

        library
    }

    fn find(&self, key: &str, character: &str, root: &Path) -> Option<PathBuf> {
        let wanted = split_character_name(key).map(|(_, component)| component);

        // 1. The same component on the same character is the strongest match.
        if let Some(wanted) = wanted.as_deref().filter(|component| !component.is_empty()) {
            if let Some(entry) = self.entries.iter().find(|entry| {
                entry.character == character
                    && entry.component.as_deref() == Some(wanted)
                    && !entry.path.starts_with(root)
            }) {
                return Some(entry.path.clone());
            }
        }

        // 2. Body-sheet parts (legs, arms, hands) share the body atlas.
        if is_body_group(wanted.as_deref()) {
            if let Some(entry) = self.entries.iter().find(|entry| {
                entry.character == character
                    && entry.component.as_deref() == Some("body")
                    && !entry.is_head
                    && !entry.path.starts_with(root)
            }) {
                return Some(entry.path.clone());
            }
        }

        None
    }
}

fn collect_diffuse_files(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth > 5 || files.len() > 2000 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with('.') || name.starts_with("disabled") {
                continue;
            }
            collect_diffuse_files(&path, depth + 1, files);
        } else if file_type.is_file() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".dds") {
                files.push(path);
            }
        }
    }
}

/// Resolve a resource file relative to its INI, then by name anywhere in the mod.
fn resolve_file(root: &Path, resource: &IniResource) -> Option<PathBuf> {
    let name = resource.filename.as_deref()?.replace('\\', "/");

    if let Some(base) = &resource.base_dir {
        let direct = base.join(&name);
        if direct.is_file() {
            return Some(direct);
        }
    }

    let basename = name.rsplit('/').next()?.to_ascii_lowercase();
    find_by_name(root, &basename, 0)
}

fn dedupe_resources_by_path(root: &Path, resources: &mut Vec<IniResource>) {
    // Key on section *and* path: the same file may be declared under several
    // variant sections (`[ResourceBodyPosition.0]`…`.3`), and collapsing those
    // would erase the section identity the toggle ranking needs. A true
    // duplicate is the same section declared by two INIs.
    let mut seen: Vec<(String, PathBuf)> = Vec::new();
    resources.retain(|resource| {
        let path = resolve_file(root, resource).or_else(|| {
            resource
                .filename
                .as_deref()
                .map(|name| PathBuf::from(name.replace('\\', "/").to_ascii_lowercase()))
        });

        let Some(path) = path else {
            return true;
        };
        let key = (resource.section.to_ascii_lowercase(), path);
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
}

fn find_by_name(dir: &Path, wanted: &str, depth: usize) -> Option<PathBuf> {
    if depth > 5 {
        return None;
    }

    let entries = fs::read_dir(dir).ok()?;
    let mut subdirectories = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_file() && name == wanted {
            return Some(entry.path());
        }
        if file_type.is_dir() && !name.starts_with('.') {
            subdirectories.push(entry.path());
        }
    }

    for directory in subdirectories {
        if let Some(found) = find_by_name(&directory, wanted, depth + 1) {
            return Some(found);
        }
    }

    None
}

fn read_limited(path: &Path) -> AppResult<Vec<u8>> {
    let metadata = fs::metadata(path)?;

    if metadata.len() > MAX_RESOURCE_BYTES {
        return Err(AppError::Preview(format!(
            "{} is larger than {} MB.",
            path.display(),
            MAX_RESOURCE_BYTES / (1024 * 1024)
        )));
    }

    Ok(fs::read(path)?)
}

#[cfg(test)]
mod texture_tests {
    use super::*;

    fn parsed_ini(text: &str) -> ParsedIni {
        (
            "body.ini".to_string(),
            PathBuf::from("body.ini"),
            parser::split_lines(text),
        )
    }

    /// State seeded from `[Constants]` exactly as `build_preview` does.
    fn base_state(parsed: &[ParsedIni]) -> ToggleState {
        let mut state = ToggleState::default();
        for (_, _, lines) in parsed {
            variants::collect_defaults(lines, &mut state);
        }
        state
    }

    #[test]
    fn exposes_only_toggles_that_change_the_model() {
        let parsed = vec![parsed_ini(
            "[Constants]\r\n\
             global persist $hide_hat = 0\r\n\
             global persist $unused = 0\r\n\
             [TextureOverrideHat]\r\n\
             if $hide_hat == 0\r\n\
             \tvb0 = ResourceHatPosition\r\n\
             \tib = ResourceHatIB\r\n\
             \tdrawindexed = 3, 0, 0\r\n\
             else\r\n\
             \tvb0 = ResourceHatHidden\r\n\
             \tib = ResourceHatHiddenIB\r\n\
             \tdrawindexed = 3, 0, 0\r\n\
             endif\r\n\
             if $unused == 1\r\n\
             \t; the branch has no binding, so the toggle changes nothing\r\n\
             endif\r\n",
        )];
        let state = base_state(&parsed);
        let harvest = harvest_all(&parsed, &state);

        let toggles = supported_toggles(&parsed, &state, &harvest);
        assert_eq!(toggles.len(), 1, "{toggles:?}");
        let toggle = &toggles[0];
        assert_eq!(toggle.name, "hide_hat");
        assert_eq!(toggle.label, "Hide hat");
        assert_eq!(toggle.value, "0");
        assert_eq!(toggle.options, vec!["0".to_string(), "1".to_string()]);
    }

    #[test]
    fn an_override_flips_the_variant_the_preview_draws() {
        let parsed = vec![parsed_ini(
            "[Constants]\r\n\
             global persist $hide_hat = 0\r\n\
             [TextureOverrideHat]\r\n\
             if $hide_hat == 0\r\n\
             \tvb0 = ResourceHatOn\r\n\
             else\r\n\
             \tvb0 = ResourceHatOff\r\n\
             endif\r\n",
        )];
        let mut state = base_state(&parsed);
        let gate: BTreeSet<String> = ["hide_hat".to_string()].into_iter().collect();

        apply_overrides(
            &mut state,
            &[PreviewToggleValue {
                name: "Hide_Hat".to_string(),
                value: "1".to_string(),
            }],
            &gate,
        );
        assert_eq!(state.value("hide_hat"), Some("1"));

        // A name the mod does not gate on is ignored instead of mutating state.
        apply_overrides(
            &mut state,
            &[PreviewToggleValue {
                name: "draw_type".to_string(),
                value: "0".to_string(),
            }],
            &gate,
        );
        assert_eq!(state.value("draw_type"), None);

        let harvest = harvest_all(&parsed, &state);
        assert!(harvest.active.contains("resourcehatoff"));
        assert!(harvest.hides("ResourceHatOn"));
    }

    #[test]
    fn completed_boolean_pair_makes_a_single_branch_toggle_exposed() {
        // Only `0` is written: the `else` branch is the implicit `1`.
        let parsed = vec![parsed_ini(
            "[Constants]\r\n\
             global persist $show_weapon = 0\r\n\
             [TextureOverrideWeapon]\r\n\
             if $show_weapon == 0\r\n\
             \tvb0 = ResourceWeaponHidden\r\n\
             else\r\n\
             \tvb0 = ResourceWeaponShown\r\n\
             endif\r\n",
        )];
        let state = base_state(&parsed);
        let harvest = harvest_all(&parsed, &state);

        let toggles = supported_toggles(&parsed, &state, &harvest);
        assert_eq!(toggles.len(), 1);
        assert_eq!(toggles[0].options, vec!["0".to_string(), "1".to_string()]);
    }

    #[test]
    fn build_preview_honours_a_toggle_override() {
        let mods = std::env::temp_dir().join(format!(
            "zzmm-toggle-preview-{}",
            std::process::id()
        ));
        let root = mods.join("ToggleWeaponMod");
        let _ = std::fs::remove_dir_all(&mods);
        std::fs::create_dir_all(&root).expect("temp mod dir");

        let vertex_buffer = |vertices: usize| {
            let mut bytes = Vec::with_capacity(vertices * 40);
            for index in 0..vertices {
                let base = index as f32 * 0.1;
                for value in [base, 0.0, base] {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                bytes.extend_from_slice(&[0u8; 28]);
            }
            bytes
        };
        let index_buffer = |indices: &[u16]| -> Vec<u8> {
            indices.iter().flat_map(|index| index.to_le_bytes()).collect()
        };

        std::fs::write(root.join("WeaponOffPosition.buf"), vertex_buffer(4)).expect("off vb");
        std::fs::write(root.join("WeaponOnPosition.buf"), vertex_buffer(8)).expect("on vb");
        std::fs::write(root.join("WeaponOff.ib"), index_buffer(&[0, 1, 2, 1, 2, 3]))
            .expect("off ib");
        std::fs::write(root.join("WeaponOn.ib"), index_buffer(&[0, 1, 2, 3, 4, 5]))
            .expect("on ib");
        std::fs::write(
            root.join("weapon.ini"),
            "[Constants]\r\n\
             global persist $show_weapon = 0\r\n\
             \r\n\
             [ResourceWeaponOffPosition]\r\n\
             filename = WeaponOffPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceWeaponOnPosition]\r\n\
             filename = WeaponOnPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceWeaponOffIB]\r\n\
             filename = WeaponOff.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [ResourceWeaponOnIB]\r\n\
             filename = WeaponOn.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [TextureOverrideWeapon]\r\n\
             hash = abc\r\n\
             if $show_weapon == 0\r\n\
             \tvb0 = ResourceWeaponOffPosition\r\n\
             \tib = ResourceWeaponOffIB\r\n\
             \tdrawindexed = 6, 0, 0\r\n\
             else\r\n\
             \tvb0 = ResourceWeaponOnPosition\r\n\
             \tib = ResourceWeaponOnIB\r\n\
             \tdrawindexed = 6, 0, 0\r\n\
             endif\r\n",
        )
        .expect("ini");

        let base = build_preview(&mods, "ToggleWeaponMod", None).expect("base preview");
        assert_eq!(base.toggles.len(), 1, "{:?}", base.toggles);
        assert_eq!(base.toggles[0].name, "show_weapon");
        assert_eq!(base.toggles[0].value, "0");
        assert_eq!(base.parts.len(), 1);
        assert_eq!(base.parts[0].name, "WeaponOff");
        assert_eq!(base.parts[0].vertex_count, 4);

        let toggled = build_preview(
            &mods,
            "ToggleWeaponMod",
            Some(vec![PreviewToggleValue {
                name: "Show_Weapon".to_string(),
                value: "1".to_string(),
            }]),
        )
        .expect("override preview");
        assert_eq!(toggled.parts.len(), 1);
        assert_eq!(toggled.parts[0].name, "WeaponOn");
        assert_eq!(toggled.parts[0].vertex_count, 8);
        assert_eq!(toggled.toggles[0].value, "1");

        let _ = std::fs::remove_dir_all(&mods);
    }

    fn texture(section: &str, filename: &str) -> IniResource {
        IniResource {
            section: section.to_string(),
            filename: Some(filename.to_string()),
            ..IniResource::default()
        }
    }

    #[test]
    fn an_ib_name_beats_a_sibling_sections_data_map() {
        let visible = vec![
            texture("ResourceBody1BNormalMap", "Body1BNormalMap.dds"),
            texture("ResourceBody1BDiffuse", "Body1BDiffuse.dds"),
        ];
        let index = IniResource {
            section: "ResourceBody1BIB".to_string(),
            filename: Some("Body1B.ib".to_string()),
            ..IniResource::default()
        };

        let mut warnings = Vec::new();
        let material = load_texture(
            Path::new("."),
            &visible,
            "body1b",
            Some("resourcebody1bnormmap"),
            Some(&index),
            Some(&Harvest::default()),
            &mut warnings,
        )
        .material;

        assert_eq!(material.as_deref(), Some("Body1BDiffuse.dds"));
    }

    #[test]
    fn never_uses_a_normal_map_as_the_colour_source() {
        let visible = vec![texture("ResourceKnifeNormalMap", "KnifeNormalMap.dds")];

        let mut warnings = Vec::new();
        let texture = load_texture(
            Path::new("."),
            &visible,
            "knife",
            Some("resourceknifenormalmap"),
            None,
            None,
            &mut warnings,
        );

        assert!(texture.url.is_none());
        assert!(texture.material.is_none());
    }

    #[test]
    fn resolves_a_redeclared_resource_inside_the_draws_own_ini_folder() {
        let root = Path::new(".");
        let seed_dir = root.join("Seed");
        let funnel_dir = root.join("Funnel");

        // Both sub-INIs declare `[Resource_Body_Diffuse]` with different files.
        let mut seed_body = texture("Resource_Body_Diffuse", "Tex\\body.dds");
        seed_body.base_dir = Some(seed_dir.clone());
        let mut funnel_body = texture("Resource_Body_Diffuse", "Fuyou.dds");
        funnel_body.base_dir = Some(funnel_dir);
        // Funnel sorts first, so a global first-match would pick its atlas.
        let visible = vec![&funnel_body, &seed_body];

        let chosen = named_resource(&visible, "resource_body_diffuse", Some(seed_dir.as_path()))
            .expect("same-folder definition");
        assert_eq!(chosen.filename.as_deref(), Some("Tex\\body.dds"));

        // No same-folder candidate: fall back to the global match so
        // texture-only INIs that live away from the mesh still resolve.
        let chosen = named_resource(
            &visible,
            "resource_body_diffuse",
            Some(Path::new("elsewhere")),
        )
        .expect("global fallback");
        assert_eq!(chosen.filename.as_deref(), Some("Fuyou.dds"));
    }

    #[test]
    fn skips_declared_but_missing_identity_textures() {
        let dir = std::env::temp_dir().join(format!("zzmm-tex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        // The bound atlas exists; the IB-named alternative was declared in the
        // INI but never shipped (Yuzuha's outfit).
        std::fs::write(dir.join("Tenga.dds"), b"not a dds").expect("write atlas");

        let mut body = texture("ResourceBody.Tenga", "Tenga.dds");
        body.base_dir = Some(dir.clone());
        let mut outfit = texture("ResourceYuzuhaOutfitADiffuse", "YuzuhaOutfitADiffuse.dds");
        outfit.base_dir = Some(dir.clone());
        let visible = vec![outfit, body];

        let index = IniResource {
            section: "ResourceYuzuhaOutfitAIB".to_string(),
            filename: Some("YuzuhaOutfitA.ib".to_string()),
            base_dir: Some(dir.clone()),
            ..IniResource::default()
        };

        let mut harvest = Harvest::default();
        harvest.index_texture_section.insert(
            "resourceyuzuhaoutfitaib".to_string(),
            "textureoverrideyuzuhaskina".to_string(),
        );
        harvest.index_texture.insert(
            "resourceyuzuhaoutfitaib".to_string(),
            "resourcebody.tenga".to_string(),
        );

        let mut warnings = Vec::new();
        let picked = load_texture(
            &dir,
            &visible,
            "yuzuhaoutfit",
            Some("resourcebody.tenga"),
            Some(&index),
            Some(&harvest),
            &mut warnings,
        );

        assert_eq!(picked.material.as_deref(), Some("Tenga.dds"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignores_stale_and_backup_ini_names() {
        assert!(is_ignored_ini_name("disabled_backup_1.merged.ini"));
        assert!(is_ignored_ini_name("old seed.ini"));
        assert!(is_ignored_ini_name("backup_body.ini"));
        assert!(is_ignored_ini_name("~temp.ini"));
        assert!(is_ignored_ini_name("body - copy.ini"));
        assert!(!is_ignored_ini_name("body.ini"));
        assert!(!is_ignored_ini_name("seed.ini"));
    }

    #[test]
    fn prefers_the_self_named_index_over_a_cross_draw_alias() {
        let buffers = vec![
            IniResource {
                section: "ResourceBellelegsPosition".to_string(),
                filename: Some("BellelegsPosition.buf".to_string()),
                ..IniResource::default()
            },
            IniResource {
                section: "ResourceBellelegsAIB".to_string(),
                filename: Some("BellelegsA.ib".to_string()),
                format: Some("DXGI_FORMAT_R32_UINT".to_string()),
                ..IniResource::default()
            },
            IniResource {
                section: "ResourceBellebodyAIB".to_string(),
                filename: Some("BellebodyA.ib".to_string()),
                format: Some("DXGI_FORMAT_R32_UINT".to_string()),
                ..IniResource::default()
            },
        ];

        // The body section binds the legs IB while drawing the body VB, so the
        // alias table claims the body IB belongs to the legs as well.
        let mut harvest = Harvest::default();
        harvest.index_position.insert(
            "resourcebellebodyaib".to_string(),
            "ResourceBellelegsVB".to_string(),
        );
        harvest
            .bound_indices
            .insert("resourcebellebodyaib".to_string());
        harvest
            .bound_indices
            .insert("resourcebellelegsaib".to_string());

        let plans = select_index_plan(
            Path::new("."),
            &buffers,
            "bellelegs",
            "ResourceBellelegsPosition",
            &harvest,
        );

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].resource.section, "ResourceBellelegsAIB");
    }

    #[test]
    fn splits_character_and_component_from_mesh_names() {
        assert_eq!(
            split_character_name("nekomatabody"),
            Some(("nekomata".to_string(), "body".to_string()))
        );
        assert_eq!(
            split_character_name("nekomatalegs"),
            Some(("nekomata".to_string(), "legs".to_string()))
        );
        assert_eq!(
            split_character_name("yixuancoat"),
            Some(("yixuan".to_string(), "coat".to_string()))
        );
        // A third-party prefix breaks the leading alphabetic run.
        assert_eq!(split_character_name("a_special_daytriggerhair"), None);
        assert_eq!(character_token("Yanagimod"), Some("yanagi".to_string()));
        assert_eq!(character_token("YiXuanMod"), Some("yixuan".to_string()));
    }

    #[test]
    fn library_only_borrows_body_atlases_for_body_parts() {
        let entry = |path: &str, character: &str, component: &str| TextureEntry {
            path: PathBuf::from(path),
            character: character.to_string(),
            component: (!component.is_empty()).then(|| component.to_string()),
            is_head: is_head_texture(component),
        };
        let library = TextureLibrary {
            entries: vec![
                entry("mods/skin/NekomataBodyADiffuse.dds", "nekomata", "body"),
                entry("mods/skin/YiXuanBodyADiffuse.dds", "yixuan", "body"),
            ],
        };
        let root = Path::new("mods/current");

        // Legs are drawn on the body sheet, so the sibling atlas is valid.
        assert_eq!(
            library.find("nekomatalegs", "nekomata", root),
            Some(PathBuf::from("mods/skin/NekomataBodyADiffuse.dds"))
        );
        // A coat has its own UV island; painting it with the body sheet is wrong.
        assert_eq!(library.find("yixuancoat", "yixuan", root), None);
        // Hair is never borrowed from a body sheet.
        assert_eq!(library.find("yanagihair", "yanagi", root), None);
    }
}
