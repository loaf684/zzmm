use std::collections::BTreeSet;

/// Modifier bits used for hotkey conflict detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
}

/// A 3DMigoto binding split into its parts.
///
/// Syntax (from d3dx.ini): `key = ctrl alt VK_F1`, with negative constraints
/// `no_ctrl`, `no_alt`, `no_shift`, or `no_modifiers` to require a bare key.
/// Unspecified modifiers are not required, which is why `F1` and `CTRL VK_F1`
/// can both fire on Ctrl+F1.
#[derive(Debug, Clone, Default)]
pub struct ParsedBinding {
    pub required: BTreeSet<Modifier>,
    pub forbidden: BTreeSet<Modifier>,
    /// Canonical key token, e.g. `VK_F1` or `X`.
    pub key: Option<String>,
}

impl ParsedBinding {
    pub fn is_empty(&self) -> bool {
        self.key.is_none()
    }
}

/// Map the many ways a key can be written onto one canonical token.
pub fn canonical_key(token: &str) -> String {
    let upper = token.trim().to_ascii_uppercase();

    if upper.starts_with("VK_") {
        return upper;
    }

    match upper.as_str() {
        "ESC" | "ESCAPE" => "VK_ESCAPE".to_string(),
        "RETURN" | "ENTER" => "VK_RETURN".to_string(),
        "BACK" | "BACKSPACE" => "VK_BACK".to_string(),
        "DEL" | "DELETE" => "VK_DELETE".to_string(),
        "INS" | "INSERT" => "VK_INSERT".to_string(),
        "PGUP" | "PAGEUP" | "PRIOR" => "VK_PRIOR".to_string(),
        "PGDN" | "PAGEDOWN" | "NEXT" => "VK_NEXT".to_string(),
        "UP" => "VK_UP".to_string(),
        "DOWN" => "VK_DOWN".to_string(),
        "LEFT" => "VK_LEFT".to_string(),
        "RIGHT" => "VK_RIGHT".to_string(),
        "SPACE" | "SPACEBAR" => "VK_SPACE".to_string(),
        "TAB" => "VK_TAB".to_string(),
        "CAPS" | "CAPSLOCK" => "VK_CAPITAL".to_string(),
        "PLUS" => "VK_OEM_PLUS".to_string(),
        "MINUS" => "VK_OEM_MINUS".to_string(),
        _ => upper,
    }
}

pub fn parse_binding(raw: &str) -> ParsedBinding {
    let mut parsed = ParsedBinding::default();

    for token in raw.split_whitespace() {
        let normalized = token.trim().to_ascii_uppercase().replace('-', "_");

        match normalized.as_str() {
            "CTRL" | "CONTROL" => {
                parsed.required.insert(Modifier::Ctrl);
            }
            "ALT" => {
                parsed.required.insert(Modifier::Alt);
            }
            "SHIFT" => {
                parsed.required.insert(Modifier::Shift);
            }
            "NO_MODIFIERS" | "NO_MODIFIER" => {
                parsed.forbidden.insert(Modifier::Ctrl);
                parsed.forbidden.insert(Modifier::Alt);
                parsed.forbidden.insert(Modifier::Shift);
            }
            "NO_CTRL" | "NO_CONTROL" => {
                parsed.forbidden.insert(Modifier::Ctrl);
            }
            "NO_ALT" => {
                parsed.forbidden.insert(Modifier::Alt);
            }
            "NO_SHIFT" => {
                parsed.forbidden.insert(Modifier::Shift);
            }
            // Key-exclusion tokens (`no_vk_decimal`) are rare and only refine a
            // binding; they never make two bindings safe from each other.
            other if other.starts_with("NO_VK_") => {}
            other if !other.is_empty() && parsed.key.is_none() => {
                parsed.key = Some(canonical_key(other));
            }
            _ => {}
        }
    }

    parsed
}

/// True when a single key press can satisfy both bindings.
///
/// Required modifiers are additive in 3DMigoto, so the *union* of both required
/// sets must not violate either binding's forbidden set.
pub fn can_fire_together(a: &ParsedBinding, b: &ParsedBinding) -> bool {
    let (Some(left), Some(right)) = (a.key.as_deref(), b.key.as_deref()) else {
        return false;
    };

    if left != right {
        return false;
    }

    a.required
        .union(&b.required)
        .all(|modifier| !a.forbidden.contains(modifier) && !b.forbidden.contains(modifier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_keys() {
        let binding = parse_binding("ctrl alt VK_F1");
        assert_eq!(binding.key.as_deref(), Some("VK_F1"));
        assert!(binding.required.contains(&Modifier::Ctrl));
        assert!(binding.required.contains(&Modifier::Alt));
        assert!(!binding.required.contains(&Modifier::Shift));
        assert!(binding.forbidden.is_empty());
    }

    #[test]
    fn normalizes_key_aliases() {
        assert_eq!(parse_binding("f1").key.as_deref(), Some("F1"));
        assert_eq!(parse_binding("esc").key.as_deref(), Some("VK_ESCAPE"));
        assert_eq!(parse_binding("vk_escape").key.as_deref(), Some("VK_ESCAPE"));
        assert_eq!(parse_binding("x").key.as_deref(), Some("X"));
    }

    #[test]
    fn unmodified_and_modified_bindings_can_overlap() {
        let plain = parse_binding("VK_F1");
        let ctrl = parse_binding("ctrl VK_F1");
        assert!(can_fire_together(&plain, &ctrl));
    }

    #[test]
    fn no_modifiers_excludes_modified_bindings() {
        let bare = parse_binding("no_modifiers VK_F1");
        let ctrl = parse_binding("ctrl VK_F1");
        assert!(!can_fire_together(&bare, &ctrl));

        let no_ctrl = parse_binding("no_ctrl VK_F1");
        assert!(!can_fire_together(&no_ctrl, &ctrl));
        assert!(can_fire_together(&no_ctrl, &parse_binding("alt VK_F1")));
    }

    #[test]
    fn different_keys_never_conflict() {
        assert!(!can_fire_together(
            &parse_binding("ctrl VK_F1"),
            &parse_binding("ctrl VK_F2")
        ));
        assert!(!can_fire_together(
            &parse_binding(""),
            &parse_binding("ctrl VK_F2")
        ));
    }
}
