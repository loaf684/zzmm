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
}
