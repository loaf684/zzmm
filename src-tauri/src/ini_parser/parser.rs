use crate::ini_parser::model::{HotkeyView, IniSectionView, IniSettingValue, IniVariable};

/// One physical line plus its original line ending. Keeping them apart is what
/// makes edits lossless: untouched lines are written back byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniLine {
    pub text: String,
    pub ending: String,
}

#[derive(Debug, Clone)]
pub struct SectionSpan {
    pub name: String,
    /// 0-based index of the `[Section]` line.
    pub header: usize,
    /// 0-based index one past the last line of the section.
    pub end: usize,
}

const COMMENT_PREFIXES: [char; 2] = [';', '#'];

pub fn split_lines(text: &str) -> Vec<IniLine> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] == b'\n' {
            let (end, ending) = if index > start && bytes[index - 1] == b'\r' {
                (index - 1, "\r\n")
            } else {
                (index, "\n")
            };

            lines.push(IniLine {
                text: text[start..end].to_string(),
                ending: ending.to_string(),
            });

            index += 1;
            start = index;
            continue;
        }

        index += 1;
    }

    if start < text.len() {
        lines.push(IniLine {
            text: text[start..].to_string(),
            ending: String::new(),
        });
    }

    lines
}

pub fn join_lines(lines: &[IniLine]) -> String {
    let capacity = lines
        .iter()
        .map(|line| line.text.len() + line.ending.len())
        .sum();

    let mut text = String::with_capacity(capacity);
    for line in lines {
        text.push_str(&line.text);
        text.push_str(&line.ending);
    }
    text
}

fn is_comment(trimmed: &str) -> bool {
    trimmed.starts_with(COMMENT_PREFIXES)
}

/// Cheap pre-check before parsing: files without a `[Key…]` header cannot
/// contribute a binding, so they can be skipped after reading the text.
pub fn has_key_sections(text: &str) -> bool {
    text.lines()
        .any(|line| line.trim_start().to_ascii_lowercase().starts_with("[key"))
}

fn section_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with('[') {
        return None;
    }
    let end = trimmed.find(']')?;
    if end < 2 {
        return None;
    }
    Some(trimmed[1..end].trim())
}

pub fn sections(lines: &[IniLine]) -> Vec<SectionSpan> {
    let mut found: Vec<SectionSpan> = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.text.trim();
        if is_comment(trimmed) {
            continue;
        }

        let Some(name) = section_name(line.text.as_str()) else {
            continue;
        };

        if let Some(previous) = found.last_mut() {
            previous.end = index;
        }

        found.push(SectionSpan {
            name: name.to_string(),
            header: index,
            end: lines.len(),
        });
    }

    found
}

/// `key = value` → the key text as written (trimmed) and the value span.
///
/// The span stops before a trailing `;` comment, so replacing it can never
/// swallow the comment or the whitespace that precedes it.
fn split_setting(line: &str) -> Option<(&str, usize, usize)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || is_comment(trimmed) {
        return None;
    }

    let equals = line.find('=')?;
    let key = line[..equals].trim();
    if key.is_empty() {
        return None;
    }

    let after_equals = equals + 1;
    let leading = line[after_equals..].len() - line[after_equals..].trim_start().len();
    let value_start = after_equals + leading;

    let rest = &line[value_start..];
    let comment_at = rest.find(';').unwrap_or(rest.len());
    let value = rest[..comment_at].trim_end();
    let value_end = value_start + value.len();

    Some((key, value_start, value_end))
}

pub fn replace_value(lines: &mut [IniLine], index: usize, value: &str) -> bool {
    let Some((_, start, end)) = split_setting(&lines[index].text) else {
        return false;
    };

    let text = &lines[index].text;
    let mut next = String::with_capacity(text.len() + value.len());
    next.push_str(&text[..start]);
    next.push_str(value);
    next.push_str(&text[end..]);
    lines[index].text = next;
    true
}

fn section_indent(lines: &[IniLine], section: &SectionSpan) -> String {
    lines[section.header + 1..section.end]
        .iter()
        .find_map(|line| {
            let trimmed = line.text.trim_start();
            if trimmed.is_empty() || is_comment(trimmed) || trimmed.starts_with('[') {
                return None;
            }
            let indent = &line.text[..line.text.len() - trimmed.len()];
            Some(indent.to_string())
        })
        .unwrap_or_default()
}

/// Append `key = value` at the end of a section, before trailing blank lines.
pub fn insert_setting(
    lines: &mut Vec<IniLine>,
    section: &SectionSpan,
    key: &str,
    value: &str,
    newline: &str,
) {
    let indent = section_indent(lines, section);

    let mut at = section.end;
    while at > section.header + 1 && lines[at - 1].text.trim().is_empty() {
        at -= 1;
    }

    // The previous line needs a terminator before we can add another line.
    if at > 0 && lines[at - 1].ending.is_empty() {
        lines[at - 1].ending = newline.to_string();
    }

    lines.insert(
        at,
        IniLine {
            text: format!("{indent}{key} = {value}"),
            ending: newline.to_string(),
        },
    );
}

fn normalize_action(value: &str) -> String {
    let lower = value.trim().to_ascii_lowercase();
    match lower.as_str() {
        "" => "activate".to_string(),
        "activate" | "hold" | "toggle" | "cycle" => lower,
        _ => "custom".to_string(),
    }
}

/// Extract every `[Key*]` section into the shape the editor table shows.
pub fn parse_hotkeys(lines: &[IniLine]) -> Vec<HotkeyView> {
    let mut hotkeys = Vec::new();

    for section in sections(lines) {
        let lower = section.name.to_ascii_lowercase();
        if !lower.starts_with("key") {
            continue;
        }

        let mut key_setting: Option<String> = None;
        let mut type_setting: Option<String> = None;
        let mut binding = String::new();
        let mut raw_type = String::new();
        let mut variables = Vec::new();
        let mut settings = Vec::new();

        for index in section.header + 1..section.end {
            let line = &lines[index].text;
            let Some((key, start, end)) = split_setting(line) else {
                continue;
            };
            let value = line[start..end].to_string();
            let line_number = (index + 1) as u32;

            if key.eq_ignore_ascii_case("key") {
                key_setting = Some(key.to_string());
                binding = value;
            } else if key.eq_ignore_ascii_case("type") {
                type_setting = Some(key.to_string());
                raw_type = value;
            } else if key.starts_with('$') {
                variables.push(IniVariable {
                    name: key.to_string(),
                    value,
                    line: line_number,
                });
            } else {
                settings.push(IniSettingValue {
                    key: key.to_string(),
                    value,
                    line: line_number,
                });
            }
        }

        let tokens: Vec<String> = binding
            .split_whitespace()
            .map(|token| token.to_ascii_uppercase())
            .collect();

        hotkeys.push(HotkeyView {
            section: section.name,
            line: (section.header + 1) as u32,
            key_setting,
            type_setting,
            binding: binding.trim().to_string(),
            action_type: normalize_action(&raw_type),
            has_key: !binding.trim().is_empty(),
            tokens,
            variables,
            settings,
        });
    }

    hotkeys
}

/// Key/value pairs of one section, in file order, with trailing `;` comments
/// removed. Used by the resource scanner.
pub fn section_settings(lines: &[IniLine], section: &SectionSpan) -> Vec<(String, String)> {
    (section.header + 1..section.end)
        .filter_map(|index| {
            let (key, start, end) = split_setting(&lines[index].text)?;
            Some((key.to_string(), lines[index].text[start..end].to_string()))
        })
        .collect()
}

pub fn parse_section_views(lines: &[IniLine]) -> Vec<IniSectionView> {
    sections(lines)
        .into_iter()
        .map(|section| {
            let setting_count = (section.header + 1..section.end)
                .filter(|index| split_setting(&lines[*index].text).is_some())
                .count() as u32;

            IniSectionView {
                is_hotkey: section.name.to_ascii_lowercase().starts_with("key"),
                name: section.name,
                line: (section.header + 1) as u32,
                setting_count,
            }
        })
        .collect()
}

/// Find a setting inside a section, case-insensitively.
pub fn find_setting(lines: &[IniLine], section_name: &str, key: &str) -> Option<usize> {
    let section = sections(lines)
        .into_iter()
        .find(|section| section.name.eq_ignore_ascii_case(section_name))?;

    (section.header + 1..section.end).find(|index| {
        split_setting(&lines[*index].text)
            .map(|(found, _, _)| found.eq_ignore_ascii_case(key))
            .unwrap_or(false)
    })
}

/// Preferred newline style of an existing document.
pub fn dominant_newline(lines: &[IniLine]) -> String {
    lines
        .iter()
        .find_map(|line| {
            if line.ending.is_empty() {
                None
            } else {
                Some(line.ending.clone())
            }
        })
        .unwrap_or_else(|| "\r\n".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "; header comment\r\n[KeyToggle]\r\n  key = ctrl VK_F1 ; toggle it\r\n  type = toggle\r\n  $active = 1\r\n\r\n[TextureOverrideX]\r\nhash = abc\r\n";

    #[test]
    fn round_trips_bytes_exactly() {
        let lines = split_lines(SAMPLE);
        assert_eq!(join_lines(&lines), SAMPLE);
        assert_eq!(lines[1].text, "[KeyToggle]");
        assert_eq!(lines[1].ending, "\r\n");
    }

    #[test]
    fn replaces_only_the_value_and_keeps_the_comment() {
        let mut lines = split_lines(SAMPLE);
        let index = find_setting(&lines, "keytoggle", "key").expect("setting");

        assert!(replace_value(&mut lines, index, "SHIFT VK_F2"));
        assert_eq!(lines[index].text, "  key = SHIFT VK_F2 ; toggle it");

        // Nothing else in the file changed.
        let text = join_lines(&lines);
        assert!(text.starts_with("; header comment\r\n[KeyToggle]\r\n"));
        assert!(text.ends_with("[TextureOverrideX]\r\nhash = abc\r\n"));
    }

    #[test]
    fn inserts_missing_settings_before_trailing_blanks() {
        let mut lines = split_lines(SAMPLE);
        let section = sections(&lines)
            .into_iter()
            .find(|section| section.name == "KeyToggle")
            .expect("section");

        insert_setting(&mut lines, &section, "condition", "$active == 1", "\r\n");

        let text = join_lines(&lines);
        assert!(text.contains("  condition = $active == 1\r\n\r\n[TextureOverrideX]"));
    }

    #[test]
    fn parses_hotkey_sections() {
        let lines = split_lines(SAMPLE);
        let hotkeys = parse_hotkeys(&lines);

        assert_eq!(hotkeys.len(), 1);
        let hotkey = &hotkeys[0];
        assert_eq!(hotkey.section, "KeyToggle");
        assert_eq!(hotkey.binding, "ctrl VK_F1");
        assert_eq!(hotkey.action_type, "toggle");
        assert_eq!(hotkey.variables.len(), 1);
        assert_eq!(hotkey.variables[0].name, "$active");
        assert!(hotkey.has_key);
        assert_eq!(hotkey.tokens, vec!["CTRL", "VK_F1"]);
    }

    #[test]
    fn defaults_type_to_activate() {
        let lines = split_lines("[KeyFoo]\nkey = x\n");
        let hotkeys = parse_hotkeys(&lines);
        assert_eq!(hotkeys[0].action_type, "activate");
        assert!(hotkeys[0].type_setting.is_none());
    }

    #[test]
    fn keeps_settings_that_are_not_key_type_or_variable() {
        let lines =
            split_lines("[KeyFoo]\nkey = x\ntype = cycle\ncondition = $a == 1\nback = $a\n");
        let hotkeys = parse_hotkeys(&lines);
        assert_eq!(hotkeys[0].settings.len(), 2);
        assert_eq!(hotkeys[0].settings[0].key, "condition");
    }

    #[test]
    fn prechecks_key_sections_case_insensitively() {
        assert!(has_key_sections("[KeySwap]\nkey = VK_1\n"));
        assert!(has_key_sections("  [keytoggle]\nkey = VK_2\n"));
        assert!(!has_key_sections("[TextureOverrideX]\nhash = abc\n"));
    }

    #[test]
    fn split_and_join_round_trip_every_line_layout() {
        for sample in [
            "",
            "\n",
            "\r\n",
            "\r\n\r\n",
            "no newline at the end",
            "a\nb",
            "a\r\nb\r\n",
            "mixed\nline\r\nendings\n",
            "; comment only\n",
        ] {
            assert_eq!(
                join_lines(&split_lines(sample)),
                sample,
                "round trip of {sample:?}"
            );
        }

        assert!(split_lines("").is_empty());

        let lines = split_lines("a\r\nb\nc");
        assert_eq!(lines.len(), 3);
        assert_eq!(
            (lines[0].text.as_str(), lines[0].ending.as_str()),
            ("a", "\r\n")
        );
        assert_eq!(
            (lines[1].text.as_str(), lines[1].ending.as_str()),
            ("b", "\n")
        );
        assert_eq!(
            (lines[2].text.as_str(), lines[2].ending.as_str()),
            ("c", "")
        );
    }

    #[test]
    fn section_spans_cover_only_their_own_lines() {
        let lines =
            split_lines("; lead\n[KeyA]\nkey = x\n\n[TextureOverrideB]\n[padding]\nhash = y\n");
        let found = sections(&lines);

        assert_eq!(found.len(), 3);
        assert_eq!(
            (found[0].name.as_str(), found[0].header, found[0].end),
            ("KeyA", 1, 4)
        );
        assert_eq!(
            (found[1].name.as_str(), found[1].header, found[1].end),
            ("TextureOverrideB", 4, 5)
        );
        assert_eq!(
            (found[2].name.as_str(), found[2].header, found[2].end),
            ("padding", 5, lines.len())
        );
    }

    #[test]
    fn commented_and_malformed_headers_are_not_sections() {
        let lines = split_lines("; [KeyFake]\n# [KeyAlsoFake]\n[]\n[unclosed\n[Real]\n");
        let found = sections(&lines);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Real");
        assert_eq!((found[0].header, found[0].end), (4, 5));
    }

    #[test]
    fn replacing_a_value_keeps_indent_comment_and_line_ending() {
        let mut lines = split_lines("[KeyA]\r\n\tkey   =  ctrl VK_F1   ; can be changed\r\n");
        let index = find_setting(&lines, "KeyA", "KEY").expect("setting");

        assert!(replace_value(&mut lines, index, "alt VK_F2"));
        assert_eq!(lines[index].text, "\tkey   =  alt VK_F2   ; can be changed");
        assert_eq!(
            join_lines(&lines),
            "[KeyA]\r\n\tkey   =  alt VK_F2   ; can be changed\r\n"
        );
    }

    #[test]
    fn value_replacement_refuses_lines_that_are_not_settings() {
        let mut lines = split_lines("[KeyA]\n; key = x\n\n= orphan\n[KeyB]\n");
        let before = join_lines(&lines);

        for index in 0..lines.len() {
            assert!(
                !replace_value(&mut lines, index, "y"),
                "line {index} is not a setting"
            );
        }

        assert_eq!(join_lines(&lines), before);
    }

    #[test]
    fn a_value_may_contain_equals_and_only_a_semicolon_ends_it() {
        let mut lines = split_lines("[KeyA]\nfilter = a=b\n");
        let index = find_setting(&lines, "KeyA", "filter").expect("setting");

        assert!(replace_value(&mut lines, index, "c=d"));
        assert_eq!(lines[index].text, "filter = c=d");

        // Only `;` starts a trailing comment, so `#` stays inside the value
        // span — the span `replace_value` overwrites.
        let lines = split_lines("[KeyA]\nkey = a=b # tail\n");
        let section = sections(&lines)[0].clone();
        assert_eq!(
            section_settings(&lines, &section),
            vec![("key".to_string(), "a=b # tail".to_string())]
        );
    }

    #[test]
    fn insertion_matches_section_indent_and_skips_trailing_blanks() {
        let mut lines = split_lines("[KeyA]\r\n    key = x\r\n\r\n[KeyB]\r\nkey = y\r\n");
        let section = sections(&lines)
            .into_iter()
            .find(|section| section.name == "KeyA")
            .expect("section");

        insert_setting(&mut lines, &section, "condition", "$a == 1", "\r\n");

        assert_eq!(
            join_lines(&lines),
            "[KeyA]\r\n    key = x\r\n    condition = $a == 1\r\n\r\n[KeyB]\r\nkey = y\r\n"
        );
    }

    #[test]
    fn insertion_terminates_an_unterminated_last_line() {
        let mut lines = split_lines("[KeyA]\nkey = x");
        let section = sections(&lines)[0].clone();

        insert_setting(&mut lines, &section, "type", "hold", "\n");

        assert_eq!(join_lines(&lines), "[KeyA]\nkey = x\ntype = hold\n");
    }

    #[test]
    fn insertion_into_an_empty_section_creates_the_first_setting() {
        let mut lines = split_lines("[KeyA]");
        let section = sections(&lines)[0].clone();

        insert_setting(&mut lines, &section, "key", "VK_F1", "\n");

        assert_eq!(join_lines(&lines), "[KeyA]\nkey = VK_F1\n");
    }

    #[test]
    fn dominant_newline_follows_the_first_line_ending() {
        assert_eq!(dominant_newline(&split_lines("a\nb\r\n")), "\n");
        assert_eq!(dominant_newline(&split_lines("a\r\nb\n")), "\r\n");
        assert_eq!(dominant_newline(&split_lines("[KeyA]")), "\r\n");
        assert_eq!(dominant_newline(&[]), "\r\n");
    }

    #[test]
    fn finds_settings_only_in_the_named_section() {
        let lines = split_lines("[KeyA]\nkey = a\n[KeyB]\nkey = b\ncondition = $x\n");

        assert_eq!(find_setting(&lines, "keyb", "KEY"), Some(3));
        assert_eq!(find_setting(&lines, "KeyB", "condition"), Some(4));
        assert_eq!(find_setting(&lines, "KeyA", "condition"), None);
        assert_eq!(find_setting(&lines, "Missing", "key"), None);
    }

    #[test]
    fn hotkey_view_keeps_spelling_uppercases_tokens_and_numbers_lines() {
        let lines = split_lines(
            "[keyf1]\nKey = ctrl  alt VK_F1\ntYpE = HOLD\ncondition = $on == 1\n$on = 1\n",
        );
        let hotkeys = parse_hotkeys(&lines);

        assert_eq!(hotkeys.len(), 1);
        let hotkey = &hotkeys[0];
        assert_eq!(hotkey.section, "keyf1");
        assert_eq!(hotkey.line, 1);
        assert_eq!(hotkey.key_setting.as_deref(), Some("Key"));
        assert_eq!(hotkey.type_setting.as_deref(), Some("tYpE"));
        assert_eq!(hotkey.binding, "ctrl  alt VK_F1");
        assert_eq!(hotkey.action_type, "hold");
        assert_eq!(hotkey.tokens, vec!["CTRL", "ALT", "VK_F1"]);
        assert!(hotkey.has_key);
        assert_eq!(
            (hotkey.variables[0].name.as_str(), hotkey.variables[0].line),
            ("$on", 5)
        );
        assert_eq!(
            (
                hotkey.settings[0].key.as_str(),
                hotkey.settings[0].value.as_str(),
                hotkey.settings[0].line
            ),
            ("condition", "$on == 1", 4)
        );
    }

    #[test]
    fn a_key_setting_outside_a_key_section_is_not_a_hotkey() {
        let lines =
            split_lines("[TextureOverrideX]\nkey = ctrl VK_F1\n[KeySwapC]\nkey = VK_2\n");
        let hotkeys = parse_hotkeys(&lines);

        assert_eq!(hotkeys.len(), 1);
        assert_eq!(hotkeys[0].section, "KeySwapC");
        assert_eq!(hotkeys[0].binding, "VK_2");
    }

    #[test]
    fn a_key_section_without_a_binding_is_reported_as_unbound() {
        let lines = split_lines("[KeyEmpty]\ncondition = $a == 1\n");
        let hotkeys = parse_hotkeys(&lines);

        assert_eq!(hotkeys.len(), 1);
        assert!(!hotkeys[0].has_key);
        assert!(hotkeys[0].key_setting.is_none());
        assert_eq!(hotkeys[0].binding, "");
        assert!(hotkeys[0].tokens.is_empty());
        assert_eq!(hotkeys[0].action_type, "activate");
    }

    #[test]
    fn action_type_maps_known_values_and_falls_back_to_custom() {
        assert_eq!(normalize_action(""), "activate");
        assert_eq!(normalize_action("   "), "activate");
        assert_eq!(normalize_action(" ACTIVATE "), "activate");
        assert_eq!(normalize_action("Cycle"), "cycle");
        assert_eq!(normalize_action("turbo"), "custom");
    }

    #[test]
    fn section_views_count_only_real_settings() {
        let lines = split_lines(
            "[KeyA]\nkey = x\n; note\n\n$var = 1\n[TextureOverrideB]\nhash = abc\n# tail\n",
        );
        let views = parse_section_views(&lines);

        assert_eq!(views.len(), 2);
        assert_eq!(
            (
                views[0].name.as_str(),
                views[0].line,
                views[0].setting_count,
                views[0].is_hotkey
            ),
            ("KeyA", 1, 2, true)
        );
        assert_eq!(
            (
                views[1].name.as_str(),
                views[1].line,
                views[1].setting_count,
                views[1].is_hotkey
            ),
            ("TextureOverrideB", 6, 1, false)
        );
    }

    #[test]
    fn section_settings_keep_order_and_drop_trailing_comments() {
        let lines = split_lines("[KeyA]\nb = 2 ; note\n\n; skip\na = 1\n");
        let section = sections(&lines)[0].clone();

        assert_eq!(
            section_settings(&lines, &section),
            vec![
                ("b".to_string(), "2".to_string()),
                ("a".to_string(), "1".to_string())
            ]
        );
    }

    #[test]
    fn precheck_ignores_comments_and_other_sections() {
        assert!(has_key_sections("[KEYF1]\n"));
        assert!(has_key_sections("\t[keytoggle]\n"));
        assert!(!has_key_sections("; [KeyF1]\n[TextureOverrideX]\nhash = abc\n"));
        assert!(!has_key_sections(""));
    }
}
