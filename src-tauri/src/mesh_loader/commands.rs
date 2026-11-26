use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};

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
        // `base_dir` keeps the INI *file* spelling: ranking (`in_root`,
        // `root_rank`) and the texture-scope comparisons read it. Resolution
        // uses `IniResource::resolve_dir`, which `scan_lines` derives from this
        // path, so a relative `filename` is read next to the INI that declared
        // it without touching those consumers.
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
    // One decode per colour map for the whole preview, however many parts bind it.
    let mut texture_cache = TextureCache::default();

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
                    &mut texture_cache,
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
                    &mut texture_cache,
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
                        texture_match = finish_texture_path(&path, &mut warnings, &mut texture_cache);
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

/// Apply the viewer's toggle overrides.
///
/// Two filters keep a stale or malformed control from perturbing the preview:
/// the name must be a variable the mod actually gates a binding on, and the
/// value must be a numeric literal (the viewer only offers numeric options).
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
        // Normalize (`1.0` → `1`) so an override compares equal to the values
        // the INI declares.
        let Some(value) = variants::numeric_literal(&entry.value) else {
            continue;
        };
        state.set(&name, &value);
    }
}

/// Candidate toggle variables and the numeric values the INI gives them,
/// merged across every INI of the mod.
///
/// A variable is a candidate only when a `[Key*]` section binds it: the key
/// editor's own notion of a user toggle. Internal flags (a `[Constants]`
/// default, a `[Present]`-derived guard, a condition-only knob) can change the
/// mesh too, but the user has no in-game control for them, so they must never
/// appear as a preview control. Values still come from every assignment and
/// comparison in the mod, so a key's option list can live apart from the
/// `[Constants]` default it overrides.
fn toggle_candidates(
    parsed: &[ParsedIni],
    state: &ToggleState,
) -> BTreeMap<String, BTreeSet<String>> {
    let declared: BTreeSet<String> = parsed
        .iter()
        .flat_map(|(_, _, lines)| variants::key_toggle_variables(lines))
        .collect();

    let mut candidates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for name in parsed
        .iter()
        .flat_map(|(_, _, lines)| variants::condition_variables(lines))
        .filter(|name| declared.contains(name))
    {
        let values = candidates.entry(name.clone()).or_default();
        for (_, _, lines) in parsed {
            values.extend(variants::candidate_values(lines, &name));
        }
    }
    for (name, values) in &mut candidates {
        if let Some(current) = state.value(name).and_then(variants::numeric_literal) {
            values.insert(current);
        }
        // 3DMigoto's boolean idiom writes only one branch (`if $x == 0 ... else
        // ...`), so a lone `0` or `1` is completed to the pair. A selector that
        // already lists several values keeps exactly its declared domain.
        if values.len() == 1 {
            if values.contains("0") {
                values.insert("1".to_string());
            } else if values.contains("1") {
                values.insert("0".to_string());
            }
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

/// A copy of the state as `build_preview` leaves it after applying one toggle
/// override: the override goes on before `[Present]` replay (so a selector
/// propagates to its derived guards) and again after it (so a direct override
/// of a derived guard wins). Probing without this replay would miss every
/// selector whose visible effect is mediated by `[Present]` and hide its
/// control.
fn probe_state(
    state: &ToggleState,
    parsed: &[ParsedIni],
    name: &str,
    value: &str,
) -> ToggleState {
    let mut probe = state.clone();
    probe.set(name, value);
    for (_, _, lines) in parsed {
        variants::apply_present_assignments(lines, &mut probe);
    }
    probe.set(name, value);
    probe
}

/// Variables whose value actually changes what the preview draws.
///
/// A candidate is a `$variable` used in an `if` / `elif` / `else if` condition
/// with at least two known numeric values *and* declared by a `$name = ...`
/// assignment inside a `[Key*]` section. Internal/derived flags (a
/// `[Constants]` default, `[Present]` guards, condition-only knobs) change the
/// mesh too but are not in-game controls, so they are never offered. Beyond
/// that gate, a candidate is exposed only when at least one alternative value
/// produces a different harvest fingerprint than the state the model is
/// rendered with, so a control appears exactly when it changes the
/// mesh, its parts, the colour map resolved for a part/index buffer, or the draw
/// ranges the game submits. Data maps (normal/light/glow/...) and null
/// placeholders are not part of the fingerprint, so a toggle that only rebinds
/// one of those earns no control. Variables that gate nothing on screen are
/// never returned.
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
                let probe = probe_state(state, parsed, &name, option);
                variants::harvest_signature(&harvest_all(parsed, &probe)) != baseline
            });
        if !affects {
            continue;
        }

        toggles.push(PreviewToggle {
            label: toggle_label(&name),
            name,
            value: current,
            options: sorted_options(options),
        });
    }

    toggles
}

/// Numeric values in value order (`2` before `10`), falling back to text order
/// for any non-numeric option.
fn sorted_options(options: BTreeSet<String>) -> Vec<String> {
    let mut values: Vec<String> = options.into_iter().collect();
    values.sort_by(|a, b| match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(a), Ok(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.cmp(b),
    });
    values
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

/// Decoded colour maps, keyed by the file they were read from. A mod that binds
/// the same atlas to many sub-draws would otherwise decode and PNG-encode it
/// once per part. One installed mod binds three 8192x8192 BC7 maps to nearly
/// every draw, which made a single preview take over seven minutes.
#[derive(Default)]
struct TextureCache {
    entries: HashMap<PathBuf, Result<texture::PreviewTexture, String>>,
}

impl TextureCache {
    fn load(&mut self, path: &Path) -> Result<texture::PreviewTexture, String> {
        let key = path.to_path_buf();
        if let Some(cached) = self.entries.get(&key) {
            return cached.clone();
        }

        let loaded = texture::load_preview_texture(path).map_err(|error| error.to_string());
        self.entries.insert(key, loaded.clone());
        loaded
    }
}

fn load_texture(
    root: &Path,
    textures: &[IniResource],
    key: &str,
    preferred: Option<&str>,
    index: Option<&IniResource>,
    harvest: Option<&Harvest>,
    warnings: &mut Vec<String>,
    cache: &mut TextureCache,
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
            return finish_texture(root, chosen, warnings, cache);
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
            return finish_texture(root, chosen, warnings, cache);
        }
    }

    // 3. The explicit binding (diffuse/ps-t0/`this`) after the own-identity
    //    check, so a section that draws a sibling mesh cannot leak its colour
    //    map onto this one.
    if let Some(chosen) = preferred_texture {
        return finish_texture(root, chosen, warnings, cache);
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
                    return finish_texture(root, chosen, warnings, cache);
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
                return finish_texture(root, chosen, warnings, cache);
            }
        }

        if let Some(harvest) = harvest {
            if let Some((_, target)) = harvest.links.iter().find(|(mesh, _)| {
                resources::cores_match(&resources::component_core(mesh), &ib_core)
            }) {
                if let Some(chosen) = named_resource(&visible, target, texture_scope) {
                    if !is_data_map(&chosen.lower_stem()) {
                        return finish_texture(root, chosen, warnings, cache);
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
        return finish_texture(root, chosen, warnings, cache);
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

fn finish_texture(
    root: &Path,
    chosen: &IniResource,
    warnings: &mut Vec<String>,
    cache: &mut TextureCache,
) -> TextureMatch {
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

    match cache.load(&path) {
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
                error: Some(error),
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
    cache: &mut TextureCache,
) -> TextureMatch {
    let candidates: Vec<&IniResource> = textures
        .iter()
        .filter(|texture| texture.filename.is_some())
        .collect();

    if let Some(chosen) = named_resource(&candidates, name, scope) {
        return finish_texture(root, chosen, warnings, cache);
    }

    warnings.push(format!(
        "{label}: this draw uses a texture captured from the running game ({name}) that is not stored on disk; rendered with the neutral material."
    ));
    TextureMatch::default()
}

fn finish_texture_path(
    path: &Path,
    warnings: &mut Vec<String>,
    cache: &mut TextureCache,
) -> TextureMatch {
    let material = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string());

    match cache.load(path) {
        Ok(preview) => TextureMatch {
            url: Some(preview.data_url),
            error: None,
            material,
            has_alpha: preview.has_alpha,
        },
        Err(error) => {
            warnings.push(format!("Could not decode {}: {error}", path.display()));
            TextureMatch {
                error: Some(error),
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

/// Normalise `folder.join(name)` and require the result to stay inside `root`.
/// `.` and `..` components are resolved lexically, so a declared
/// `..\textures\body\Blue.dds` is accepted when it stays under the mod folder
/// and refused the moment it climbs above it. Absolute and rooted paths are
/// only accepted when they already point inside `root`. Returns the normalised
/// path; the caller still checks that it is a real, symlink-confined file.
fn path_inside(root: &Path, folder: &Path, name: &str) -> Option<PathBuf> {
    let joined = folder.join(name);
    let remainder = joined.strip_prefix(root).ok()?;

    let mut depth: Vec<std::ffi::OsString> = Vec::new();
    for component in remainder.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if depth.pop().is_none() {
                    return None;
                }
            }
            Component::Normal(part) => depth.push(part.to_os_string()),
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }

    let mut normalized = root.to_path_buf();
    for part in depth {
        normalized.push(part);
    }
    Some(normalized)
}

/// An existing file that is inside `root` once symlinks and junctions are
/// followed. `Path::starts_with` compares whole components, so a sibling folder
/// such as `<root>2` never counts as being inside `<root>`. The canonical path
/// is returned so later comparisons use one spelling.
fn confined_file(root: &Path, candidate: &Path) -> Option<PathBuf> {
    if !candidate.is_file() {
        return None;
    }

    let file = fs::canonicalize(candidate).ok()?;
    let root = fs::canonicalize(root).ok()?;
    file.starts_with(root).then_some(file)
}

/// Resolve a resource file inside the mod folder.
///
/// 3DMigoto resolves a relative `filename` against the folder of the INI that
/// declares it, which is `resolve_dir` (a folder scan records the folder it
/// walked). A name that carries a directory is a path: it resolves only in that
/// folder or as the same path from the mod root, never by bare name, so a
/// missing `.\Variant.02\Body.ib` cannot silently become a same-named file from
/// a sibling variant. A bare file name is a mod-relative reference and is
/// searched for in the whole mod. Every result must be a real file inside the
/// mod folder.
fn resolve_file(root: &Path, resource: &IniResource) -> Option<PathBuf> {
    let name = resource.filename.as_deref()?.replace('\\', "/");
    let folder = resource
        .resolve_dir
        .as_deref()
        .or(resource.base_dir.as_deref());

    if let Some(folder) = folder {
        if let Some(candidate) = path_inside(root, folder, &name) {
            if let Some(found) = confined_file(root, &candidate) {
                return Some(found);
            }
        }
    }

    if name.contains('/') {
        // An INI in a subfolder may also write the path from the mod root
        // (`Model/body.buf`). That is the declared path again, not a basename
        // substitution, and it still has to stay inside the mod folder.
        if let Some(candidate) = path_inside(root, root, &name) {
            if let Some(found) = confined_file(root, &candidate) {
                return Some(found);
            }
        }

        return None;
    }

    confined_file(root, &find_by_name(root, &name.to_ascii_lowercase(), 0)?)
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

    // Iterate in a stable order: the fallback picks one file among same-named
    // copies, and a read_dir order that varies between machines would make the
    // same mod render differently.
    subdirectories.sort();

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

/// End-to-end checks against a synthetic mod folder: real INI text, real
/// `.buf`/`.ib`/`.dds` bytes on disk, through the same `build_preview` the UI
/// calls. Unit tests on the individual decoders cannot catch a wrong resource
/// classification, texture pick, or part split.
#[cfg(test)]
mod pipeline_tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    struct Scratch {
        root: PathBuf,
        folder: String,
        dir: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static NEXT: AtomicU32 = AtomicU32::new(0);

            // Tests run in parallel, so each scratch folder needs its own root.
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "zzmm-preview-{}-{unique}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            let dir = root.join(name);
            fs::create_dir_all(&dir).expect("mod folder");
            Self {
                root,
                folder: name.to_string(),
                dir,
            }
        }

        fn write(&self, relative: &str, bytes: &[u8]) {
            let path = self.dir.join(relative.replace('/', "\\"));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent folder");
            }
            fs::write(path, bytes).expect("write file");
        }

        fn write_text(&self, relative: &str, text: &str) {
            self.write(relative, text.as_bytes());
        }

        fn build(&self) -> AppResult<MeshPreview> {
            build_preview(&self.root, &self.folder, None)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// Positions packed into 40-byte ZZZ-style vertices with zero padding.
    fn position_buf(vertices: &[[f32; 3]]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for vertex in vertices {
            for value in vertex {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&[0u8; 28]);
        }
        bytes
    }

    fn u16_indices(indices: &[u16]) -> Vec<u8> {
        indices.iter().flat_map(|i| i.to_le_bytes()).collect()
    }

    /// Uncompressed 2x2 BGRA8 DDS with explicit masks, so `parse` sees the
    /// same pixel format a hand-made texture swap ships.
    fn bgra_dds(pixels: &[[u8; 4]; 4]) -> Vec<u8> {
        let mut bytes = vec![0u8; 128];
        bytes[0..4].copy_from_slice(b"DDS ");
        bytes[4..8].copy_from_slice(&124u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&2u32.to_le_bytes());
        bytes[76..80].copy_from_slice(&8u32.to_le_bytes());
        bytes[84..88].copy_from_slice(&[0, 0, 0, 0]);
        bytes[88..92].copy_from_slice(&32u32.to_le_bytes());
        bytes[92..96].copy_from_slice(&0x00FF_0000u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&0x0000_FF00u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&0x0000_00FFu32.to_le_bytes());
        bytes[104..108].copy_from_slice(&0xFF00_0000u32.to_le_bytes());
        for &[r, g, b, a] in pixels {
            bytes.extend_from_slice(&[b, g, r, a]);
        }
        bytes
    }

    fn decode<T: Copy>(base64: &str, cast: fn([u8; 4]) -> T) -> Vec<T> {
        STANDARD
            .decode(base64)
            .expect("valid base64")
            .chunks_exact(4)
            .map(|chunk| cast([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect()
    }

    fn positions(part: &MeshPart) -> Vec<[f32; 3]> {
        decode(&part.positions, f32::from_le_bytes)
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect()
    }

    fn indices(part: &MeshPart) -> Vec<u32> {
        decode(&part.indices, u32::from_le_bytes)
    }

    const BASIC_INI: &str = "\
[ResourceMyPropPosition]
filename = MyPropPosition.buf
stride = 40

[ResourceMyPropTexcoord]
filename = MyPropTexcoord.buf
stride = 20

[ResourceMyPropIB]
filename = MyProp.ib
format = DXGI_FORMAT_R16_UINT

[ResourceMyPropDiffuse]
filename = MyPropDiffuse.dds

[TextureOverrideMyProp]
hash = 12345678
vb0 = ResourceMyPropPosition
ib = ResourceMyPropIB
ps-t0 = ResourceMyPropDiffuse
drawindexed = 3, 0, 0
";

    fn basic_mod() -> Scratch {
        let scratch = Scratch::new("MyProp");
        scratch.write_text("MyProp.ini", BASIC_INI);
        scratch.write(
            "MyPropPosition.buf",
            &position_buf(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
        );
        // f32 UV pairs at the start of a 20-byte texcoord vertex.
        let mut texcoord = Vec::new();
        for uv in [[0.0f32, 0.0f32], [0.5, 0.25], [1.0, 1.0]] {
            for value in uv {
                texcoord.extend_from_slice(&value.to_le_bytes());
            }
            texcoord.extend_from_slice(&[0u8; 12]);
        }
        scratch.write("MyPropTexcoord.buf", &texcoord);
        scratch.write("MyProp.ib", &u16_indices(&[0, 1, 2]));
        scratch.write(
            "MyPropDiffuse.dds",
            &bgra_dds(&[[255, 0, 0, 255]; 4]),
        );
        scratch
    }


    #[test]
    fn builds_one_textured_part_from_a_declared_mod() {
        let scratch = basic_mod();
        let preview = scratch.build().expect("preview");

        assert_eq!(preview.parts.len(), 1);
        let part = &preview.parts[0];
        assert_eq!(part.vertex_count, 3);
        assert_eq!(part.triangle_count, 1);
        assert_eq!(part.material.as_deref(), Some("MyPropDiffuse.dds"));
        assert!(part.texture.is_some(), "diffuse was not decoded");
        assert!(!part.has_alpha);
        assert!(!preview.toggles_applied);

        assert_eq!(
            positions(part),
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        assert_eq!(indices(part), vec![0, 1, 2]);
        // The UV pair must come from the texcoord buffer, not zeros.
        let uvs = decode(&part.uvs, f32::from_le_bytes);
        assert_eq!(uvs, vec![0.0, 0.0, 0.5, 0.25, 1.0, 1.0]);
        // Normals are computed in Rust and normalised.
        let normals = decode(&part.normals, f32::from_le_bytes);
        assert_eq!(normals.len(), 9);
        let length = (normals[0] * normals[0] + normals[1] * normals[1] + normals[2] * normals[2]).sqrt();
        assert!((length - 1.0).abs() < 1e-5);
    }

    #[test]
    fn flags_alpha_cutouts_of_a_partial_alpha_texture() {
        let scratch = basic_mod();
        // One transparent texel makes the whole diffuse a cutout candidate, the
        // way hair/lace atlases are packed.
        scratch.write(
            "MyPropDiffuse.dds",
            &bgra_dds(&[[255, 0, 0, 255], [0, 255, 0, 0], [0, 0, 255, 255], [255, 255, 0, 255]]),
        );

        let preview = scratch.build().expect("preview");
        assert!(preview.parts[0].has_alpha);
    }

    #[test]
    fn ignores_manager_backups_that_would_duplicate_geometry() {
        let scratch = basic_mod();
        // Same INI under a backup name: stacking it would render the triangle
        // twice with stale resource paths.
        scratch.write_text("DISABLED_MyProp.ini", BASIC_INI);
        scratch.write_text("MyProp.ini.bak", BASIC_INI);

        let preview = scratch.build().expect("preview");
        assert_eq!(preview.parts.len(), 1);
        assert_eq!(preview.parts[0].triangle_count, 1);
    }
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
             [KeyHideHat]\r\n\
             key = VK_1\r\n\
             type = cycle\r\n\
             $hide_hat = 0, 1\r\n\
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
    fn exposes_a_model_changing_key_bound_toggle() {
        // A `[Key*]` cycle that only moves a draw range still changes the
        // submitted triangles: the model-output predicate must keep exposing it.
        let parsed = vec![parsed_ini(
            "[Constants]\r\n\
             global persist $shoes = 0\r\n\
             [KeyShoes]\r\n\
             key = VK_1\r\n\
             type = cycle\r\n\
             $shoes = 0, 1\r\n\
             [TextureOverrideShoes]\r\n\
             if $shoes == 1\r\n\
             \tib = ResourceShoesIB\r\n\
             \tdrawindexed = 6, 6, 0\r\n\
             else\r\n\
             \tib = ResourceNoShoesIB\r\n\
             \tdrawindexed = 6, 0, 0\r\n\
             endif\r\n",
        )];
        let state = base_state(&parsed);
        let harvest = harvest_all(&parsed, &state);

        let toggles = supported_toggles(&parsed, &state, &harvest);
        assert_eq!(toggles.len(), 1, "{toggles:?}");
        assert_eq!(toggles[0].name, "shoes");
        assert_eq!(toggles[0].value, "0");
        assert_eq!(toggles[0].options, vec!["0".to_string(), "1".to_string()]);
    }

    #[test]
    fn excludes_model_changing_internal_condition_flags() {
        // BelleStars_Toggles shape: `$bear` and `$starshairdraw` are
        // [Constants]/condition flags that switch geometry, but no [Key*]
        // section binds them, so the viewer must not offer them as in-game
        // controls. `$noop` is key-bound yet changes nothing, proving the
        // model-output predicate still filters candidates after the new gate.
        let parsed = vec![parsed_ini(
            "[Constants]\r\n\
             global persist $bear = 0\r\n\
             global persist $starshairdraw = 1\r\n\
             global persist $noop = 0\r\n\
             [KeyNoop]\r\n\
             key = VK_2\r\n\
             type = cycle\r\n\
             $noop = 0, 1\r\n\
             [TextureOverrideBear]\r\n\
             if $bear == 1\r\n\
             \tvb0 = ResourceBearPosition\r\n\
             else\r\n\
             \tvb0 = ResourceBodyPosition\r\n\
             endif\r\n\
             [TextureOverrideHair]\r\n\
             if $starshairdraw == 1\r\n\
             \tvb0 = ResourceStarHairPosition\r\n\
             endif\r\n\
             if $noop == 1\r\n\
             \t; no binding: the key-bound variable changes nothing\r\n\
             endif\r\n",
        )];
        let state = base_state(&parsed);
        let harvest = harvest_all(&parsed, &state);

        let toggles = supported_toggles(&parsed, &state, &harvest);
        assert!(toggles.is_empty(), "{toggles:?}");
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

        // A non-numeric value cannot be one of the viewer's numeric options.
        apply_overrides(
            &mut state,
            &[PreviewToggleValue {
                name: "hide_hat".to_string(),
                value: "on".to_string(),
            }],
            &gate,
        );
        assert_eq!(state.value("hide_hat"), Some("1"));

        let harvest = harvest_all(&parsed, &state);
        assert!(harvest.active.contains("resourcehatoff"));
        assert!(harvest.hides("ResourceHatOn"));
    }

    #[test]
    fn cycle_toggle_keeps_its_constants_default_and_sorts_numerically() {
        // The exact reproduction from the paired-branch review: a cycle writes
        // only its on-values, while the [Constants] default is the off state.
        let parsed = vec![parsed_ini(
            "[Constants]\r\n\
             global persist $swapkey = 0\r\n\
             [KeyCycle]\r\n\
             key = VK_3\r\n\
             type = cycle\r\n\
             $swapkey = 2, 10\r\n\
             [TextureOverrideBody]\r\n\
             if $swapkey == 2\r\n\
             \tvb0 = ResourceBody2\r\n\
             else if $swapkey == 10\r\n\
             \tvb0 = ResourceBody10\r\n\
             else\r\n\
             \tvb0 = ResourceBody0\r\n\
             endif\r\n",
        )];
        let state = base_state(&parsed);
        let harvest = harvest_all(&parsed, &state);

        let toggles = supported_toggles(&parsed, &state, &harvest);
        assert_eq!(toggles.len(), 1, "{toggles:?}");
        let toggle = &toggles[0];
        assert_eq!(toggle.name, "swapkey");
        assert_eq!(toggle.value, "0");
        assert_eq!(
            toggle.options,
            vec!["0".to_string(), "2".to_string(), "10".to_string()]
        );
        assert!(
            toggle.options.contains(&toggle.value),
            "currentValue must be one of options"
        );
    }

    #[test]
    fn present_derived_selector_is_exposed() {
        let parsed = vec![parsed_ini(
            "[Constants]\r\n\
             global persist $outfit = 0\r\n\
             global persist $outfit_on = 0\r\n\
             [KeyOutfit]\r\n\
             key = VK_1\r\n\
             type = cycle\r\n\
             $outfit = 0, 1\r\n\
             [Present]\r\n\
             if $outfit == 1\r\n\
             \t$outfit_on = 1\r\n\
             else\r\n\
             \t$outfit_on = 0\r\n\
             endif\r\n\
             [TextureOverrideBody]\r\n\
             hash = abc\r\n\
             if $outfit_on == 1\r\n\
             \tvb0 = ResourceOutfit1\r\n\
             else\r\n\
             \tvb0 = ResourceOutfit0\r\n\
             endif\r\n",
        )];
        let mut state = base_state(&parsed);
        // `$outfit_on` is only derived by `[Present]`, exactly as in the build.
        for (_, _, lines) in &parsed {
            variants::apply_present_assignments(lines, &mut state);
        }
        assert_eq!(state.value("outfit_on"), Some("0"));
        let harvest = harvest_all(&parsed, &state);
        assert!(harvest.active.contains("resourceoutfit0"));

        let toggles = supported_toggles(&parsed, &state, &harvest);
        let outfit = toggles
            .iter()
            .find(|toggle| toggle.name == "outfit")
            .expect("Present-mediated selector must be exposed");
        assert_eq!(outfit.value, "0");
        assert_eq!(outfit.options, vec!["0".to_string(), "1".to_string()]);
    }

    #[test]
    fn completed_boolean_pair_makes_a_single_branch_toggle_exposed() {
        // Only `0` is written: the `else` branch is the implicit `1`.
        let parsed = vec![parsed_ini(
            "[Constants]\r\n\
             global persist $show_weapon = 0\r\n\
             [KeyShowWeapon]\r\n\
             key = VK_1\r\n\
             type = toggle\r\n\
             $show_weapon = 0\r\n\
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
             [KeyShowWeapon]\r\n\
             key = VK_1\r\n\
             type = cycle\r\n\
             $show_weapon = 0, 1\r\n\
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
            &mut TextureCache::default(),
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
            &mut TextureCache::default(),
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
            &mut TextureCache::default(),
        );

        assert_eq!(picked.material.as_deref(), Some("Tenga.dds"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decodes_each_colour_map_only_once() {
        let dir = std::env::temp_dir().join(format!("zzmm-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        // A 2x2 uncompressed BGRA DDS, so the first load really decodes pixels.
        let mut dds = vec![0u8; 128];
        dds[0..4].copy_from_slice(b"DDS ");
        dds[4..8].copy_from_slice(&124u32.to_le_bytes());
        dds[12..16].copy_from_slice(&2u32.to_le_bytes());
        dds[16..20].copy_from_slice(&2u32.to_le_bytes());
        dds[88..92].copy_from_slice(&32u32.to_le_bytes());
        dds[92..96].copy_from_slice(&0x00FF_0000u32.to_le_bytes());
        dds[96..100].copy_from_slice(&0x0000_FF00u32.to_le_bytes());
        dds[100..104].copy_from_slice(&0x0000_00FFu32.to_le_bytes());
        dds[104..108].copy_from_slice(&0xFF00_0000u32.to_le_bytes());
        dds.extend_from_slice(&[0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255]);

        let path = dir.join("SharedAtlas.dds");
        std::fs::write(&path, &dds).expect("write dds");

        // The same atlas is bound by every part of a mesh; it must be decoded
        // and encoded once, not once per part.
        let mut cache = TextureCache::default();
        let first = cache.load(&path).expect("first load");
        let second = cache.load(&path).expect("second load");

        assert_eq!(first.data_url, second.data_url);
        assert_eq!(cache.entries.len(), 1);

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

/// Resource `filename` resolution. 3DMigoto resolves a declared file against
/// the folder of the INI that declares it; a same-named file living in another
/// variant's folder must never be substituted for it.
#[cfg(test)]
mod resource_path_tests {
    use super::*;

    fn vertex_buffer(vertices: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(vertices * 40);
        for index in 0..vertices {
            let base = index as f32 * 0.1;
            for value in [base, 0.0, base] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&[0u8; 28]);
        }
        bytes
    }

    fn index_buffer(indices: &[u16]) -> Vec<u8> {
        indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect()
    }

    fn temp_mods(name: &str) -> PathBuf {
        let mods = std::env::temp_dir().join(format!("zzmm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&mods);
        mods
    }

    #[test]
    fn a_relative_path_resolves_inside_the_declaring_inis_folder() {
        // Both variants ship the same file names; only the declared variant's
        // files may be used. The previous resolution joined the path onto the
        // INI *file* and then searched the mod by name, which returned
        // `Variant.01`'s buffer for the active `Variant.02` draw.
        let mods = temp_mods("variant-paths");
        let root = mods.join("VariantMod");
        for variant in ["Variant.01", "Variant.02"] {
            std::fs::create_dir_all(root.join(variant)).expect("variant dir");
        }
        std::fs::write(root.join("Variant.01/BodyPosition.buf"), vertex_buffer(4)).expect("vb 1");
        std::fs::write(root.join("Variant.02/BodyPosition.buf"), vertex_buffer(8)).expect("vb 2");
        std::fs::write(root.join("Variant.01/Body.ib"), index_buffer(&[0, 1, 2, 1, 2, 3]))
            .expect("ib 1");
        std::fs::write(
            root.join("Variant.02/Body.ib"),
            index_buffer(&[0, 1, 2, 1, 2, 3, 4, 5, 6, 7, 4, 6]),
        )
        .expect("ib 2");
        std::fs::write(
            root.join("variant.ini"),
            "[Constants]\r\n\
             global persist $v = 1\r\n\
             \r\n\
             [ResourceBodyPosition.0]\r\n\
             filename = .\\Variant.01\\BodyPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceBodyPosition.1]\r\n\
             filename = .\\Variant.02\\BodyPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceBodyIB.0]\r\n\
             filename = .\\Variant.01\\Body.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [ResourceBodyIB.1]\r\n\
             filename = .\\Variant.02\\Body.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [TextureOverrideBody]\r\n\
             hash = abc\r\n\
             if $v == 0\r\n\
             \tvb0 = ResourceBodyPosition.0\r\n\
             \tib = ResourceBodyIB.0\r\n\
             \tdrawindexed = 6, 0, 0\r\n\
             else\r\n\
             \tvb0 = ResourceBodyPosition.1\r\n\
             \tib = ResourceBodyIB.1\r\n\
             \tdrawindexed = 12, 0, 0\r\n\
             endif\r\n",
        )
        .expect("ini");

        let preview = build_preview(&mods, "VariantMod", None).expect("preview");
        assert_eq!(preview.parts.len(), 1);
        assert_eq!(preview.parts[0].vertex_count, 8, "active variant's position buffer");
        assert_eq!(preview.parts[0].triangle_count, 4, "active variant's index buffer");

        // The other variant still resolves to its own files.
        let other = build_preview(
            &mods,
            "VariantMod",
            Some(vec![PreviewToggleValue {
                name: "v".to_string(),
                value: "0".to_string(),
            }]),
        )
        .expect("preview");
        assert_eq!(other.parts[0].vertex_count, 4);
        assert_eq!(other.parts[0].triangle_count, 2);

        let _ = std::fs::remove_dir_all(&mods);
    }

    #[test]
    fn a_missing_relative_path_is_reported_and_never_substituted() {
        // `Variant.02` ships no `Body.ib`; `Variant.01`'s must not stand in for
        // it. The head slice keeps the preview alive so the warning is
        // observable on a successful build.
        let mods = temp_mods("missing-path");
        let root = mods.join("MissingMod");
        std::fs::create_dir_all(root.join("Variant.01")).expect("variant 1");
        std::fs::create_dir_all(root.join("Variant.02")).expect("variant 2");
        std::fs::write(root.join("Variant.02/BodyPosition.buf"), vertex_buffer(9)).expect("vb");
        std::fs::write(root.join("Variant.01/Body.ib"), index_buffer(&[0, 1, 2, 1, 2, 3]))
            .expect("foreign ib");
        std::fs::write(root.join("Variant.02/HeadPosition.buf"), vertex_buffer(3)).expect("head vb");
        std::fs::write(root.join("Variant.02/Head.ib"), index_buffer(&[0, 1, 2])).expect("head ib");
        std::fs::write(
            root.join("missing.ini"),
            "[ResourceBodyPosition]\r\n\
             filename = .\\Variant.02\\BodyPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceBodyIB]\r\n\
             filename = .\\Variant.02\\Body.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [ResourceHeadPosition]\r\n\
             filename = .\\Variant.02\\HeadPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceHeadIB]\r\n\
             filename = .\\Variant.02\\Head.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [TextureOverrideBody]\r\n\
             hash = abc\r\n\
             vb0 = ResourceBodyPosition\r\n\
             ib = ResourceBodyIB\r\n\
             drawindexed = 6, 0, 0\r\n\
             \r\n\
             [TextureOverrideHead]\r\n\
             hash = def\r\n\
             vb0 = ResourceHeadPosition\r\n\
             ib = ResourceHeadIB\r\n\
             drawindexed = 3, 0, 0\r\n",
        )
        .expect("ini");

        let preview = build_preview(&mods, "MissingMod", None).expect("preview");
        assert!(
            preview
                .warnings
                .iter()
                .any(|warning| warning.contains("Body.ib was not found on disk")),
            "missing buffer must be reported: {:?}",
            preview.warnings
        );
        assert_eq!(preview.parts.len(), 1, "{:?}", preview.parts);
        assert_eq!(preview.parts[0].name, "Head");
        assert!(
            preview.parts.iter().all(|part| part.triangle_count != 2),
            "the sibling variant's index buffer must not be used"
        );

        let _ = std::fs::remove_dir_all(&mods);
    }

    #[test]
    fn a_bare_file_name_still_resolves_from_a_nested_ini() {
        // An INI in a subfolder may name a file that lives beside the mod root;
        // that documented fallback must survive the stricter path handling.
        let mods = temp_mods("bare-name");
        let root = mods.join("BareMod");
        std::fs::create_dir_all(root.join("sub")).expect("sub dir");
        std::fs::write(root.join("SharedPosition.buf"), vertex_buffer(4)).expect("vb");
        std::fs::write(root.join("Shared.ib"), index_buffer(&[0, 1, 2, 1, 2, 3])).expect("ib");
        std::fs::write(
            root.join("sub/part.ini"),
            "[ResourceSharedPosition]\r\n\
             filename = SharedPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceSharedIB]\r\n\
             filename = Shared.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [TextureOverrideShared]\r\n\
             hash = abc\r\n\
             vb0 = ResourceSharedPosition\r\n\
             ib = ResourceSharedIB\r\n\
             drawindexed = 6, 0, 0\r\n",
        )
        .expect("ini");

        let preview = build_preview(&mods, "BareMod", None).expect("preview");
        assert_eq!(preview.parts.len(), 1);
        assert_eq!(preview.parts[0].vertex_count, 4);
        assert_eq!(preview.parts[0].triangle_count, 2);

        let _ = std::fs::remove_dir_all(&mods);
    }

    #[test]
    fn a_nested_ini_resolves_a_path_written_from_the_mod_root() {
        // The INI lives in `sub/`, so its own folder is the first base; the
        // declared `Model/...` path is written from the mod root instead. Both
        // readings are the declared path, so the second one must be tried.
        let mods = temp_mods("nested-ini");
        let root = mods.join("NestedMod");
        std::fs::create_dir_all(root.join("sub")).expect("sub dir");
        std::fs::create_dir_all(root.join("Model")).expect("model dir");
        std::fs::write(root.join("Model/BodyPosition.buf"), vertex_buffer(6)).expect("vb");
        std::fs::write(
            root.join("Model/BodyIndex.ib"),
            index_buffer(&[0, 1, 2, 1, 2, 3, 4, 5, 3]),
        )
        .expect("ib");
        std::fs::write(
            root.join("sub/part.ini"),
            "[ResourceBodyPosition]\r\n\
             filename = Model/BodyPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceBodyIB]\r\n\
             filename = Model/BodyIndex.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [TextureOverrideBody]\r\n\
             hash = abc\r\n\
             vb0 = ResourceBodyPosition\r\n\
             ib = ResourceBodyIB\r\n\
             drawindexed = 9, 0, 0\r\n",
        )
        .expect("ini");

        let preview = build_preview(&mods, "NestedMod", None).expect("preview");
        assert_eq!(preview.parts.len(), 1);
        assert_eq!(preview.parts[0].vertex_count, 6);
        assert_eq!(preview.parts[0].triangle_count, 3);
        assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);

        let _ = std::fs::remove_dir_all(&mods);
    }

    #[test]
    fn a_parent_step_that_stays_inside_the_mod_still_resolves() {
        // Installed mods reach shared files one level up
        // (`resources/..\textures\body\Blue.dds`); containment must allow that
        // while still refusing a step above the mod folder.
        let mods = temp_mods("parent-step");
        let root = mods.join("ParentMod");
        std::fs::create_dir_all(root.join("resources")).expect("ini dir");
        std::fs::create_dir_all(root.join("Model")).expect("model dir");
        std::fs::write(root.join("Model/BodyPosition.buf"), vertex_buffer(6)).expect("vb");
        std::fs::write(
            root.join("Model/BodyIndex.ib"),
            index_buffer(&[0, 1, 2, 1, 2, 3, 4, 5, 3]),
        )
        .expect("ib");
        std::fs::write(
            root.join("resources/part.ini"),
            "[ResourceBodyPosition]\r\n\
             filename = ..\\Model\\BodyPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceBodyIB]\r\n\
             filename = ..\\Model\\BodyIndex.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [TextureOverrideBody]\r\n\
             hash = abc\r\n\
             vb0 = ResourceBodyPosition\r\n\
             ib = ResourceBodyIB\r\n\
             drawindexed = 9, 0, 0\r\n",
        )
        .expect("ini");

        let preview = build_preview(&mods, "ParentMod", None).expect("preview");
        assert_eq!(preview.parts.len(), 1);
        assert_eq!(preview.parts[0].vertex_count, 6);
        assert_eq!(preview.parts[0].triangle_count, 3);
        assert!(preview.warnings.is_empty(), "{:?}", preview.warnings);

        let _ = std::fs::remove_dir_all(&mods);
    }

    #[test]
    fn path_normalisation_refuses_steps_above_the_mod_folder() {
        let root = Path::new("/mods/Mod");
        let resources = root.join("resources");

        // A step up from a subfolder stays inside the mod.
        assert_eq!(
            path_inside(root, &resources, "../textures/body/Blue.dds"),
            Some(root.join("textures/body/Blue.dds"))
        );
        // `.` segments and mixed steps normalise.
        assert_eq!(
            path_inside(root, &resources, "./.././Model/Body.buf"),
            Some(root.join("Model/Body.buf"))
        );
        // Climbing above the mod folder is refused.
        assert_eq!(path_inside(root, root, "../Leaked.buf"), None);
        assert_eq!(path_inside(root, &resources, "../../Leaked.buf"), None);
        // An absolute path outside the mod is refused.
        assert_eq!(path_inside(root, root, "C:/elsewhere/Leaked.buf"), None);
    }

    #[test]
    fn a_path_that_leaves_the_mod_folder_is_refused() {
        // The declared path walks out of the mod and a real file sits there.
        // The preview must not read it, and must say the buffer is missing.
        let mods = temp_mods("escape-path");
        let root = mods.join("EscapeMod");
        std::fs::create_dir_all(&root).expect("mod dir");
        std::fs::write(mods.join("LeakedPosition.buf"), vertex_buffer(9)).expect("outside vb");
        std::fs::write(root.join("HeadPosition.buf"), vertex_buffer(3)).expect("head vb");
        std::fs::write(root.join("Head.ib"), index_buffer(&[0, 1, 2])).expect("head ib");
        std::fs::write(
            root.join("escape.ini"),
            "[ResourceLeakedPosition]\r\n\
             filename = ..\\LeakedPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceHeadPosition]\r\n\
             filename = HeadPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceHeadIB]\r\n\
             filename = Head.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [TextureOverrideLeaked]\r\n\
             hash = abc\r\n\
             vb0 = ResourceLeakedPosition\r\n\
             draw = 9, 0\r\n\
             \r\n\
             [TextureOverrideHead]\r\n\
             hash = def\r\n\
             vb0 = ResourceHeadPosition\r\n\
             ib = ResourceHeadIB\r\n\
             drawindexed = 3, 0, 0\r\n",
        )
        .expect("ini");

        let preview = build_preview(&mods, "EscapeMod", None).expect("preview");
        assert!(
            preview.warnings.iter().any(|warning| warning
                .contains("LeakedPosition.buf was not found on disk")),
            "the refused path must be reported: {:?}",
            preview.warnings
        );
        assert_eq!(preview.parts.len(), 1, "{:?}", preview.parts);
        assert_eq!(preview.parts[0].name, "Head");
        assert!(
            preview
                .parts
                .iter()
                .all(|part| part.vertex_count != 9 && part.triangle_count != 3),
            "a file outside the mod folder must never be read"
        );

        let _ = std::fs::remove_dir_all(&mods);
    }

    #[test]
    fn a_missing_path_is_never_filled_by_a_same_named_root_file() {
        // The declared `.\Variant.02\Body.ib` is absent; a same-named file sits
        // at the mod root. The path must not fall back to it.
        let mods = temp_mods("root-basename");
        let root = mods.join("RootNameMod");
        std::fs::create_dir_all(root.join("Variant.02")).expect("variant dir");
        std::fs::write(root.join("Variant.02/BodyPosition.buf"), vertex_buffer(9)).expect("vb");
        std::fs::write(root.join("Body.ib"), index_buffer(&[0, 1, 2, 1, 2, 3]))
            .expect("root ib");
        std::fs::write(root.join("HeadPosition.buf"), vertex_buffer(3)).expect("head vb");
        std::fs::write(root.join("Head.ib"), index_buffer(&[0, 1, 2])).expect("head ib");
        std::fs::write(
            root.join("rootname.ini"),
            "[ResourceBodyPosition]\r\n\
             filename = .\\Variant.02\\BodyPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceBodyIB]\r\n\
             filename = .\\Variant.02\\Body.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [ResourceHeadPosition]\r\n\
             filename = HeadPosition.buf\r\n\
             stride = 40\r\n\
             \r\n\
             [ResourceHeadIB]\r\n\
             filename = Head.ib\r\n\
             format = DXGI_FORMAT_R16_UINT\r\n\
             \r\n\
             [TextureOverrideBody]\r\n\
             hash = abc\r\n\
             vb0 = ResourceBodyPosition\r\n\
             ib = ResourceBodyIB\r\n\
             drawindexed = 6, 0, 0\r\n\
             \r\n\
             [TextureOverrideHead]\r\n\
             hash = def\r\n\
             vb0 = ResourceHeadPosition\r\n\
             ib = ResourceHeadIB\r\n\
             drawindexed = 3, 0, 0\r\n",
        )
        .expect("ini");

        let preview = build_preview(&mods, "RootNameMod", None).expect("preview");
        assert!(
            preview
                .warnings
                .iter()
                .any(|warning| warning.contains("Body.ib was not found on disk")),
            "missing path must be reported: {:?}",
            preview.warnings
        );
        assert_eq!(preview.parts.len(), 1, "{:?}", preview.parts);
        assert_eq!(preview.parts[0].name, "Head");
        assert!(
            preview.parts.iter().all(|part| part.triangle_count != 2),
            "the root-level same-named file must not stand in for the missing path"
        );

        let _ = std::fs::remove_dir_all(&mods);
    }
}
