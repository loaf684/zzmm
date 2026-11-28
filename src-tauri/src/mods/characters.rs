use crate::mods::metadata::ModMetadata;

/// Best-effort roster used only when metadata does not name a character. Mod
/// folders are hand-named, so aliases cover both full names and the short forms
/// commonly used by creators (`EllenMod`, `Zhuyuanmod`, `S11Mod`, ...).
const ROSTER: &[(&str, &[&str])] = &[
    (
        "Soldier 0 Anby",
        &[
            "soldier 0 anby",
            "soldier 0",
            "s0 anby",
            "anby s0",
            "anbys0",
        ],
    ),
    ("Soldier 11", &["soldier 11", "s11"]),
    ("Ellen Joe", &["ellen joe", "ellen"]),
    ("Zhu Yuan", &["zhu yuan", "zhuyuan"]),
    ("Nicole Demara", &["nicole demara", "nicole"]),
    ("Anby Demara", &["anby demara", "anby"]),
    ("Billy Kid", &["billy kid", "billy"]),
    ("Corin Wickes", &["corin wickes", "corin"]),
    ("Caesar King", &["caesar king", "caesar"]),
    ("Burnice White", &["burnice white", "burnice"]),
    ("Jane Doe", &["jane doe", "jane"]),
    ("Qingyi", &["qingyi", "qing yi"]),
    ("Seth Lowell", &["seth lowell", "seth"]),
    ("Grace Howard", &["grace howard", "grace"]),
    ("Rina", &["rina"]),
    ("Von Lycaon", &["von lycaon", "lycaon"]),
    ("Koleda Belobog", &["koleda belobog", "koleda"]),
    ("Ben Bigger", &["ben bigger", "ben"]),
    ("Anton Ivanov", &["anton ivanov", "anton"]),
    ("Soukaku", &["soukaku"]),
    ("Astra Yao", &["astra yao", "astra"]),
    ("Evelyn Chevalier", &["evelyn chevalier", "evelyn"]),
    ("Hoshimi Miyabi", &["hoshimi miyabi", "miyabi"]),
    ("Asaba Harumasa", &["asaba harumasa", "harumasa"]),
    ("Tsukishiro Yanagi", &["tsukishiro yanagi", "yanagi"]),
    ("Lighter", &["lighter"]),
    ("Trigger", &["trigger"]),
    ("Vivian Banshee", &["vivian banshee", "vivian"]),
    ("Hugo Vlad", &["hugo vlad", "hugo"]),
    ("Yixuan", &["yixuan", "yi xuan"]),
    ("Ju Fufu", &["ju fufu", "jufufu"]),
    ("Alice Thymefield", &["alice thymefield", "alice"]),
    ("Pulchra Fellini", &["pulchra fellini", "pulchra"]),
    ("Orphie Magus", &["orphie magus", "orphie"]),
    ("Lucia", &["lucia"]),
    ("Yidhari", &["yidhari"]),
    ("Banyue", &["banyue"]),
    ("Manato", &["manato"]),
    ("Komano", &["komano"]),
    ("Seed", &["seed"]),
    ("Pan Yinhu", &["pan yinhu"]),
    ("Piper Wheel", &["piper wheel", "piper"]),
    ("Lucy", &["lucy"]),
    ("Nekomata", &["nekomata", "nekomiya mana", "nekomiya"]),
    ("Yuzuha", &["yuzuha"]),
    ("Belle", &["belle"]),
    ("Wise", &["wise"]),
    ("Aria", &["aria"]),
    ("Cissia", &["cissia"]),
    ("Claret", &["claret"]),
    ("Dialyn", &["dialyn"]),
    ("Nangong", &["nangong"]),
    ("Norma", &["norma"]),
    ("Phoenix", &["phoenix"]),
    ("Promeia", &["promeia"]),
    ("Remielle", &["remielle"]),
    ("Roxi", &["roxi", "roxy"]),
    ("Sigrid", &["sigrid"]),
    ("Sunna", &["sunna"]),
    ("Velina", &["velina"]),
    ("Ye Shunguang", &["ye shunguang", "yeshunguang"]),
    ("Zhao", &["zhao"]),
];

const GENERIC_TAGS: &[&str] = &[
    "zzz",
    "zenless",
    "zenless zone zero",
    "mod",
    "mods",
    "skin",
    "skins",
    "outfit",
    "3dmigoto",
    "xxmi",
    "zzmi",
    "wip",
    "new",
    "custom",
    "character",
    "characters",
    "texture",
    "preview",
    "uncategorized",
    "unknown",
    "other",
];

/// Words creators glue onto a character name (`EllenMod`, `Roxymod`) or append
/// as separate camel-case tokens (`AliceModMerged`). Used only to accept a token
/// that starts with a roster alias, never to match arbitrary substrings.
const TOKEN_SUFFIXES: &[&str] = &[
    "mods", "mod", "merged", "merge", "skins", "skin", "outfits", "outfit", "custom", "zzz",
    "xxmi", "3dmigoto",
];

/// GameBanana's own root-category names for the two non-character targets.
/// Reusing them keeps Installed grouping and the Browse category filter using
/// one vocabulary.
pub const UI_CATEGORY: &str = "UI";
pub const BANGBOO_CATEGORY: &str = "Bangboo Skins";

#[cfg_attr(not(test), allow(dead_code))]
pub fn guess(metadata: Option<&ModMetadata>, base_name: &str) -> String {
    guess_with_context(metadata, base_name, base_name, &[])
}

/// Like [`guess`], but also considers the displayed mod name and INI/folder
/// hints collected while scanning. Folders are often generic (`Outfit 01`)
/// while the INI inside still says `Dialyn.ini`.
pub fn guess_with_context(
    metadata: Option<&ModMetadata>,
    base_name: &str,
    display_name: &str,
    hints: &[String],
) -> String {
    if let Some(name) = metadata.and_then(|meta| meta.character.as_deref()) {
        let name = name.trim();
        if !name.is_empty() && !is_generic(name) {
            return name.to_string();
        }
    }

    // Roster matching runs before the bracket prefix because tags such as
    // `[NSFW] Ellen Joe` or `[4K] Zhu Yuan` would otherwise be mistaken for a
    // character name and hide the real one.
    let roster = best_roster_match(base_name, display_name, hints);

    // UI and Bangboo mods are not character skins. Only the folder and display
    // name are safe hints: character skins routinely ship a `ui.ini`, so INI
    // names would misclassify them.
    if let Some(category) = category_target(base_name, display_name, roster.is_some()) {
        return category.to_string();
    }

    if let Some((character, _)) = roster {
        return character.to_string();
    }

    from_bracket_prefix(base_name)
        .or_else(|| from_bracket_prefix(display_name))
        .unwrap_or_else(|| "Uncategorized".to_string())
}

/// Recognize the two non-character targets GameBanana exposes as root
/// categories. A UI token wins even beside a character name (`UI Reskin
/// Phoenix` is a UI mod, not a Phoenix skin). A Bangboo token beside a confident
/// roster name is ambiguous — a skin can simply feature a Bangboo — so the
/// character is kept there.
fn category_target(
    base_name: &str,
    display_name: &str,
    has_character: bool,
) -> Option<&'static str> {
    let mut ui = false;
    let mut bangboo = false;

    for name in [base_name, display_name] {
        for token in tokenize(name) {
            if token_matches_alias(&token, "ui")
                || token_matches_alias(&token, "hud")
                || token == "interface"
            {
                ui = true;
            } else if token == "bangboos" || token_matches_alias(&token, "bangboo") {
                bangboo = true;
            }
        }
    }

    if ui {
        return Some(UI_CATEGORY);
    }

    if bangboo && !has_character {
        return Some(BANGBOO_CATEGORY);
    }

    None
}

/// The character the app is *sure* about, for deciding whether a freshly
/// installed mod collides with one that is already there. Unlike
/// [`guess_with_context`], the result is only ever a known ZZZ roster name
/// found in the sidecar metadata, the folder name, the display name, or an INI
/// hint.
///
/// Free-form tags (`[4K]`, `[NSFW]`) and custom user labels are deliberately
/// not "sure": treating them as characters would disable new mods for no
/// reason, so an unidentified or ambiguous mod reports `None`.
pub fn confident_character(
    metadata: Option<&ModMetadata>,
    base_name: &str,
    display_name: &str,
    hints: &[String],
) -> Option<&'static str> {
    if let Some(name) = metadata.and_then(|meta| meta.character.as_deref()) {
        let name = name.trim();
        if is_non_character_category(name) {
            return None;
        }
        if !name.is_empty() && !is_generic(name) {
            if let Some((character, _)) = roster_match(name) {
                return Some(character);
            }
        }
    }

    let roster = best_roster_match(base_name, display_name, hints);

    // A UI or Bangboo mod is not a character skin even when its title names one
    // (`UI Reskin Phoenix`). Collision detection must not disable it.
    if category_target(base_name, display_name, roster.is_some()).is_some() {
        return None;
    }

    roster.map(|(character, _)| character)
}

/// The two automatic category labels are targets, not roster characters, so
/// they must never participate in per-character collision decisions.
fn is_non_character_category(name: &str) -> bool {
    name.eq_ignore_ascii_case(UI_CATEGORY) || name.eq_ignore_ascii_case(BANGBOO_CATEGORY)
}

/// Longest roster alias found across the folder name, the display name and the
/// INI hints. `AnbyS0` must resolve to Soldier 0 Anby, not Anby Demara.
fn best_roster_match(
    base_name: &str,
    display_name: &str,
    hints: &[String],
) -> Option<(&'static str, usize)> {
    let mut best: Option<(&'static str, usize)> = None;

    for hint in std::iter::once(base_name)
        .chain(std::iter::once(display_name))
        .chain(hints.iter().map(String::as_str))
    {
        if let Some((character, length)) = roster_match(hint) {
            if best.map_or(true, |(_, best_length)| length > best_length) {
                best = Some((character, length));
            }
        }
    }

    best
}

fn from_bracket_prefix(name: &str) -> Option<String> {
    for (open, close) in [('[', ']'), ('【', '】')] {
        let Some(open_at) = name.find(open) else {
            continue;
        };
        let start = open_at + open.len_utf8();
        let Some(close_at) = name[start..].find(close) else {
            continue;
        };
        let inner = clean(name[start..start + close_at].trim());

        if !inner.is_empty() && !is_generic(&inner) {
            return Some(inner);
        }
    }

    None
}

fn is_generic(value: &str) -> bool {
    let normalized = normalize(value);
    GENERIC_TAGS.contains(&normalized.as_str())
}

/// Returns the roster display name and the length of the matched alias so the
/// longest match wins (`AnbyS0` is Soldier 0 Anby, not Anby Demara).
fn roster_match(name: &str) -> Option<(&'static str, usize)> {
    let tokens = tokenize(name);
    if tokens.is_empty() {
        return None;
    }

    let mut best: Option<(&'static str, usize)> = None;

    for (display, aliases) in ROSTER {
        for alias in *aliases {
            let alias_tokens = tokenize(alias);
            if alias_tokens.is_empty() {
                continue;
            }

            let matched = if alias_tokens.len() == 1 {
                tokens
                    .iter()
                    .any(|token| token_matches_alias(token, &alias_tokens[0]))
            } else {
                // `EllenJoe`, `NicoleDemara` and `AstraYao` keep the full name
                // in one token, so also test the alias with its spaces removed.
                let glued = alias_tokens.concat();
                tokens
                    .iter()
                    .any(|token| token_matches_alias(token, &glued))
                    || tokens.windows(alias_tokens.len()).any(|window| {
                        window
                            .iter()
                            .zip(&alias_tokens)
                            .all(|(token, alias_token)| token == alias_token)
                    })
            };

            if matched {
                let length = alias_tokens.iter().map(String::len).sum::<usize>();
                if best.map_or(true, |(_, best_length)| length > best_length) {
                    best = Some((display, length));
                }
            }
        }
    }

    best
}

fn token_matches_alias(token: &str, alias: &str) -> bool {
    if token == alias {
        return true;
    }

    let Some(mut rest) = token.strip_prefix(alias) else {
        return false;
    };

    // Peel generic suffixes so `nicolemod`, `roxymod` and `zhuyuanmod` match.
    loop {
        if rest.is_empty() {
            return true;
        }

        let mut stripped = false;
        for suffix in TOKEN_SUFFIXES {
            if let Some(next) = rest.strip_prefix(suffix) {
                rest = next;
                stripped = true;
                break;
            }
        }

        if !stripped {
            break;
        }
    }

    // Version tails glued straight onto the name (`NicoleDemarav2`, `Ellen01`).
    if let Some(version) = rest.strip_prefix('v').or_else(|| rest.strip_prefix('V')) {
        if !version.is_empty() && version.chars().all(|character| character.is_ascii_digit()) {
            return true;
        }
    }

    !rest.is_empty() && rest.chars().all(|character| character.is_ascii_digit())
}

fn clean(value: &str) -> String {
    value
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Splits camel-case and separator-delimited names into lowercase tokens, so
/// `AliceMod_merged` -> `["alice", "mod", "merged"]` and `AnbyS0` ->
/// `["anby", "s0"]`.
fn tokenize(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut tokens = Vec::new();
    let mut current = String::new();

    for (index, &character) in chars.iter().enumerate() {
        if !character.is_alphanumeric() {
            push_token(&mut tokens, &mut current);
            continue;
        }

        let previous = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
        let next = chars.get(index + 1).copied();
        let camel_boundary = character.is_uppercase()
            && previous.map_or(false, |value| {
                value.is_lowercase() || value.is_ascii_digit()
            });
        // Keep acronyms together but split them from a following name:
        // `ZZZEllen` -> `ZZZ` + `Ellen`.
        let acronym_boundary = character.is_uppercase()
            && previous.map_or(false, char::is_uppercase)
            && next.map_or(false, char::is_lowercase);

        if (camel_boundary || acronym_boundary) && !current.is_empty() {
            push_token(&mut tokens, &mut current);
        }

        current.push(character);
    }

    push_token(&mut tokens, &mut current);
    tokens
}

fn push_token(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
        current.clear();
    }
}

/// Lowercase, strip punctuation, and collapse separators. Used for the bracket
/// prefix and generic-tag checks.
fn normalize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_metadata_character() {
        let metadata = ModMetadata {
            character: Some("Custom Name".into()),
            ..ModMetadata::default()
        };
        assert_eq!(guess(Some(&metadata), "[Ellen Joe] Skin"), "Custom Name");
    }

    #[test]
    fn uses_bracket_prefix_before_roster() {
        assert_eq!(guess(None, "[Yixuan] Red Dress"), "Yixuan");
        assert_eq!(guess(None, "[ZZZ] Ellen Joe Streetwear"), "Ellen Joe");
        assert_eq!(guess(None, "【Yixuan】Red Dress"), "Yixuan");
    }

    #[test]
    fn matches_camel_case_and_glued_mod_suffixes() {
        assert_eq!(guess(None, "EllenMod_merge"), "Ellen Joe");
        assert_eq!(guess(None, "Nicolemod"), "Nicole Demara");
        assert_eq!(guess(None, "Zhuyuanmod"), "Zhu Yuan");
        assert_eq!(guess(None, "Roxymod"), "Roxi");
        assert_eq!(guess(None, "JuFufu-Bache v1.0"), "Ju Fufu");
        assert_eq!(guess(None, "AstraYaoMod"), "Astra Yao");
        assert_eq!(guess(None, "YiXuanMod"), "Yixuan");
    }

    #[test]
    fn prefers_the_longest_character_match() {
        assert_eq!(
            guess(None, "AnbyS0 - Leotard Soldier! v12b (QD)"),
            "Soldier 0 Anby"
        );
        assert_eq!(guess(None, "S11Mod"), "Soldier 11");
    }

    #[test]
    fn uses_display_name_and_ini_hints() {
        assert_eq!(
            guess_with_context(None, "Outfit 01", "Dialyn Summer", &[]),
            "Dialyn"
        );
        assert_eq!(
            guess_with_context(None, "Mod A", "Mod A", &["Dialyn.ini".to_string()]),
            "Dialyn"
        );
    }

    #[test]
    fn ignores_tag_prefixes_when_a_character_matches() {
        assert_eq!(guess(None, "[NSFW] Ellen Joe Streetwear"), "Ellen Joe");
        assert_eq!(guess(None, "[4K] Zhu Yuan"), "Zhu Yuan");
    }

    #[test]
    fn matches_full_names_written_without_spaces() {
        assert_eq!(guess(None, "ellenjoe_bunny"), "Ellen Joe");
        assert_eq!(guess(None, "nicoledemarav2"), "Nicole Demara");
        assert_eq!(guess(None, "astrayaomod"), "Astra Yao");
    }

    #[test]
    fn falls_back_to_uncategorized() {
        assert_eq!(guess(None, "Some Random Outfit"), "Uncategorized");
        assert_eq!(guess(None, "JiggleForgeShaderFix"), "Uncategorized");
    }

    #[test]
    fn recognizes_ui_and_bangboo_targets() {
        // UI wins beside a character name: the mod edits the interface, not
        // the character it depicts.
        assert_eq!(guess(None, "UI Reskin Phoenix"), "UI");
        assert_eq!(guess(None, "Animated NSFW Nicole Ui"), "UI");
        assert_eq!(guess(None, "HUD Overhaul"), "UI");
        assert_eq!(guess(None, "[UI] Clean Menus"), "UI");
        assert_eq!(guess(None, "Bangboo Reskin"), "Bangboo Skins");
        assert_eq!(guess(None, "ZZZ-Bangboo-tomori"), "Bangboo Skins");
        assert_eq!(
            guess_with_context(None, "Outfit 01", "Bangboo Edition", &[]),
            "Bangboo Skins"
        );
    }

    #[test]
    fn explicit_categories_and_ambiguous_names_are_not_overridden() {
        let metadata = ModMetadata {
            character: Some("Bangboo Skins".into()),
            ..ModMetadata::default()
        };
        assert_eq!(guess(Some(&metadata), "UI Pack"), "Bangboo Skins");

        // A character plus a Bangboo reference is ambiguous; keep the character.
        assert_eq!(guess(None, "Ellen Bangboo Hoodie"), "Ellen Joe");
    }

    #[test]
    fn ui_ini_names_do_not_override_a_character() {
        // Many character skins ship a `ui.ini` for their menu icon.
        assert_eq!(
            guess_with_context(None, "EllenMod", "EllenMod", &["ui.ini".to_string()]),
            "Ellen Joe"
        );
    }

    #[test]
    fn confident_character_requires_a_known_roster_name() {
        assert_eq!(
            confident_character(None, "EllenMod_merge", "EllenMod_merge", &[]),
            Some("Ellen Joe")
        );
        assert_eq!(
            confident_character(None, "Outfit 01", "Outfit 01", &["Dialyn.ini".to_string()]),
            Some("Dialyn")
        );
        // A UI mod titles a character but is not that character's skin.
        assert_eq!(
            confident_character(None, "UI Reskin Phoenix", "UI Reskin Phoenix", &[]),
            None
        );
        assert_eq!(
            confident_character(None, "Bangboo Reskin", "Bangboo Reskin", &[]),
            None
        );
        let ui = ModMetadata {
            character: Some("UI".into()),
            ..ModMetadata::default()
        };
        assert_eq!(
            confident_character(Some(&ui), "Phoenix", "Phoenix", &[]),
            None
        );
        // A `[Tag]` prefix that is not a known character is ambiguous, so it must
        // not look like a collision.
        assert_eq!(
            confident_character(None, "[4K] Something", "[4K] Something", &[]),
            None
        );
        assert_eq!(
            confident_character(None, "Some Random Outfit", "Some Random Outfit", &[]),
            None
        );
    }

    #[test]
    fn confident_character_only_trusts_a_roster_name_from_metadata() {
        let known = ModMetadata {
            character: Some("Ellen".into()),
            ..ModMetadata::default()
        };
        assert_eq!(
            confident_character(Some(&known), "Outfit 01", "Outfit 01", &[]),
            Some("Ellen Joe")
        );

        // A custom user label is not a ZZZ character, so two mods carrying it
        // must not disable each other.
        let custom = ModMetadata {
            character: Some("My Custom Label".into()),
            ..ModMetadata::default()
        };
        assert_eq!(
            confident_character(Some(&custom), "Outfit 01", "Outfit 01", &[]),
            None
        );
    }
}
