//! The rewrite engine: a line-based port of ZZZ-Mod-Fixer's `Ini` class.
//!
//! Upstream works on the whole file as one string. This port works line by line
//! on [`IniLine`]s (text + its original ending) so that comments, blank lines,
//! CRLF endings and every line the fix does not touch survive byte-for-byte -
//! upstream emits bare `\n` for everything it inserts, which is what the
//! "preserve CRLF" requirement rules out.
//!
//! Behaviour deliberately kept from upstream: the skip rules, the `hash = …`
//! scan, the fix commands and their placeholder protocol, the queue of hashes a
//! fix asks to revisit, idempotency (already-fixed files report nothing), and
//! buffer-level commands being *detected* rather than applied. Every deviation
//! is listed in `docs/TECHNICAL.md`.

use std::collections::{HashSet, VecDeque};

use crate::fixer::database::{is_buffer_command, Database, Entry, RawCommand};
use crate::fixer::model::{HashUpdate, SectionAddition, UnsupportedCommand};
use crate::ini_parser::parser::{dominant_newline, join_lines, split_lines, IniLine};

/// Placeholders upstream puts into generated section templates: captured
/// section content, captured indexed content, and a captured insertion offset.
/// One left over after substitution means the mod's INI is not shaped the way
/// the fix expects, so the section is not written at all.
const PLACEHOLDER_CONTENT: char = '\u{1F370}'; // 🍰
const PLACEHOLDER_INDEXED: char = '\u{1F90D}'; // 🤍
const PLACEHOLDER_POSITION: char = '\u{1F332}'; // 🌲

const SKIN_TEXTURE_RUN: &str = "CommandListSkinTexture";

pub const REASON_BUFFER: &str = "requires the buffer pass";
pub const REASON_UNKNOWN: &str = "unknown command";
pub const REASON_UNPORTED: &str = "not ported";
pub const REASON_MALFORMED: &str = "malformed command arguments";
pub const REASON_SUBSTITUTION: &str = "section content substitution failed";
pub const REASON_CYCLE: &str = "update cycle in the hash database; left unchanged";

/// Commands the engine implements. `database.rs` asserts that the shipped asset
/// declares nothing outside this set, so a regenerated asset cannot silently
/// grow a command nobody ported. Test-only: at run time the engine matches the
/// same names and reports anything else, so production never has to ask.
#[cfg(test)]
const HANDLED: [&str; 8] = [
    "log",
    "update_hash",
    "add_section_if_missing",
    "multiply_section_if_missing",
    "add_ib_check_if_missing",
    "update_buffer_blend_indices",
    "zzz_12_shrink_texcoord_color",
    "zzz_13_remap_texcoord",
];

/// Known upstream commands this port does not run. They are reported, never
/// skipped: upstream's `transfer_indexed_sections` reads a field its own
/// constructor cannot accept, and the data never calls the rest.
const UNPORTED: [&str; 4] = [
    "transfer_indexed_sections",
    "comment_sections",
    "comment_commandlists",
    "remove_section",
];

#[cfg(test)]
pub fn handles(command: &str) -> bool {
    HANDLED.contains(&command)
}

/// True when the engine will not be surprised by a command name: it either
/// implements it or reports it on purpose.
#[cfg(test)]
pub fn knows(command: &str) -> bool {
    handles(command) || UNPORTED.contains(&command)
}

#[derive(Debug, Default, Clone)]
pub struct RunReport {
    /// True when the file content changed; false means "write nothing".
    pub touched: bool,
    pub updated: Vec<HashUpdate>,
    pub additions: Vec<SectionAddition>,
    pub unsupported: Vec<UnsupportedCommand>,
    /// Character modules owning the hashes found in the file, in file order.
    pub characters: Vec<String>,
    pub needs_buffer_pass: bool,
    /// Set when the run was aborted; the file is then left untouched.
    pub error: Option<String>,
}

impl RunReport {
    pub fn character(&self) -> String {
        self.characters
            .first()
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string())
    }
}

pub struct RunOutcome {
    pub report: RunReport,
    /// Rewritten text; identical to the input when `report.touched` is false.
    pub text: String,
}

/// Stable identity of an `update_hash` recommendation. Both hashes are
/// lowercased so the ignore list matches whatever case the INI used.
pub fn hash_key(from: &str, to: &str) -> String {
    format!(
        "hash:{}>{}",
        from.to_ascii_lowercase(),
        to.to_ascii_lowercase()
    )
}

/// Stable identity of an added-section recommendation.
pub fn section_key(hash: &str, section: &str) -> String {
    format!(
        "section:{}:{}",
        hash.to_ascii_lowercase(),
        section.to_ascii_lowercase()
    )
}

/// Scan one INI file's text and apply the fixes for the hashes it contains.
/// Pure: nothing is written, and the input is left alone.
pub fn run(text: &str, db: &Database) -> RunOutcome {
    run_with_ignore(text, db, &HashSet::new())
}

/// As [`run`], minus the recommendations in `ignored` (the keys
/// [`hash_key`]/[`section_key`] produce for this file).
///
/// An ignored rewrite is still reported - the UI keeps showing what the fixer
/// would do - but is never performed, and the fixes that keyed off the hash it
/// would have written are not attempted either. Scan and apply share this
/// function, so a report always says exactly what a later apply writes.
pub fn run_with_ignore(text: &str, db: &Database, ignored: &HashSet<String>) -> RunOutcome {
    let lines = split_lines(text);
    let newline = dominant_newline(&lines);

    let mut runner = Runner {
        db,
        lines,
        newline,
        present: Default::default(),
        report: RunReport::default(),
        ignored,
    };
    runner.finish();
    let outcome = RunOutcome {
        text: join_lines(&runner.lines),
        report: runner.report,
    };

    if outcome.report.error.is_some() {
        // Nothing was written, so nothing may claim to have been fixed.
        let mut report = outcome.report;
        report.touched = false;
        report.updated.clear();
        report.additions.clear();
        return RunOutcome {
            report,
            text: text.to_string(),
        };
    }

    outcome
}

// MARK: sections

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Section {
    /// Index of the `[TextureOverride…]` line.
    header: usize,
    /// Index of the section's first `hash = …` line.
    hash_line: usize,
    /// One past the last non-blank line inside the section.
    end: usize,
}

/// Captured values a fix passes to the commands it queues (`🍰`, `🌲`, …).
/// Insertion-ordered like upstream's dict, so placeholder substitution is
/// deterministic.
#[derive(Debug, Default)]
struct Data {
    entries: Vec<(String, String)>,
}

impl Data {
    fn set(&mut self, key: &str, value: String) {
        match self.entries.iter_mut().find(|(name, _)| name == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key.to_string(), value)),
        }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug)]
enum Queued {
    CaptureSection {
        content: Option<String>,
        position: Option<String>,
    },
    CreateNewSection {
        section_content: String,
        saved_position: Option<String>,
    },
}

/// Per-hash fix state, mirroring upstream's `DefaultArgs`.
#[derive(Debug)]
struct Ctx {
    hash: String,
    label: String,
    data: Data,
    queued_hashes: Vec<String>,
    queued: VecDeque<Queued>,
    error: Option<String>,
    /// True once one of this hash's own recommendations was ignored: the rest of
    /// the bundle would only make sense after the ignored rewrite, so it is
    /// reported and skipped as well.
    ignored: bool,
}

struct Runner<'a> {
    db: &'a Database,
    lines: Vec<IniLine>,
    newline: String,
    present: std::collections::HashSet<String>,
    report: RunReport,
    /// Recommendation keys the user ignored for this file, from
    /// [`crate::fixer::store::IgnoreSet::keys_for`].
    ignored: &'a HashSet<String>,
}

impl<'a> Runner<'a> {
    fn finish(&mut self) {
        // Every hash the file mentions, in file order, plus - later - every hash
        // a fix asks to revisit. Upstream's `has_hash` answers yes for both,
        // which is what keeps "add if missing" from adding twice.
        let found = self.scan_hashes();
        self.present.extend(found.iter().cloned());

        for hash in &found {
            if let Some(entry) = self.db.get(hash) {
                if !self.report.characters.contains(&entry.character) {
                    self.report.characters.push(entry.character.clone());
                }
            }
        }

        let db = self.db;
        let mut stack = found;
        let mut done: std::collections::HashSet<String> = Default::default();

        while let Some(hash) = stack.pop() {
            if !done.insert(hash.clone()) {
                continue;
            }

            let Some(entry) = db.get(&hash) else {
                continue;
            };

            let mut ctx = Ctx {
                hash,
                label: entry.label.clone(),
                data: Data::default(),
                queued_hashes: Vec::new(),
                queued: VecDeque::new(),
                error: None,
                ignored: false,
            };
            self.execute_group(entry, &mut ctx);
            stack.extend(ctx.queued_hashes);

            if let Some(error) = ctx.error {
                if self.report.error.is_none() {
                    self.report.error = Some(error);
                }
                break;
            }
        }
    }

    /// All uncommented `hash = …` values, lowercased, in file order.
    fn scan_hashes(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(|line| hash_value(&line.text))
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .collect()
    }

    fn sections_with_hash(&self, hash: &str) -> Vec<Section> {
        let mut sections = Vec::new();
        let mut index = 0;

        while index < self.lines.len() {
            if !is_override_header(&self.lines[index].text) {
                index += 1;
                continue;
            }

            let header = index;
            let mut match_end = header + 1;
            while match_end < self.lines.len()
                && !is_override_header(&self.lines[match_end].text)
            {
                match_end += 1;
            }

            // A section is matched by its first `hash = …` line only.
            let hash_line = (header + 1..match_end)
                .find(|line| hash_value(&self.lines[*line].text).is_some());

            if let Some(hash_line) = hash_line {
                if hash_value(&self.lines[hash_line].text)
                    .is_some_and(|value| value.eq_ignore_ascii_case(hash))
                {
                    let mut end = hash_line + 1;
                    for (offset, line) in self.lines[hash_line + 1..match_end].iter().enumerate() {
                        if !line.text.trim().is_empty() {
                            end = hash_line + 2 + offset;
                        }
                    }
                    sections.push(Section {
                        header,
                        hash_line,
                        end,
                    });
                }
            }

            index = match_end;
        }

        sections
    }

    /// Upstream's `get_critical_content`: the section's lines minus its header,
    /// its `hash` line and its `match_first_index` line, rejoined with the
    /// document's own line ending.
    fn critical_content(&self, section: Section) -> String {
        let mut text = String::new();
        let mut first = true;

        for line in &self.lines[section.header..section.end] {
            if !is_critical(&line.text) {
                continue;
            }
            if !first {
                text.push_str(&self.newline);
            }
            text.push_str(&line.text);
            first = false;
        }

        text
    }

    fn insert_text(&mut self, index: usize, text: &str) {
        let index = index.min(self.lines.len());
        let mut block = split_lines(text);

        if block.is_empty() {
            return;
        }

        // Keep the block on its own lines even when the file's last line had no
        // ending of its own.
        if index > 0 && self.lines[index - 1].ending.is_empty() {
            self.lines[index - 1].ending = self.newline.clone();
        }
        if index < self.lines.len() && block.last().is_some_and(|line| line.ending.is_empty()) {
            if let Some(last) = block.last_mut() {
                last.ending = self.newline.clone();
            }
        }

        let tail = self.lines.split_off(index);
        self.lines.extend(block);
        self.lines.extend(tail);
    }

    /// Insert a fresh line under an existing one. When the line it lands under
    /// ended the file without a newline, that line gets one first - the two
    /// lines must not be glued together.
    fn insert_line_after(&mut self, index: usize, text: &str) {
        if self.lines[index].ending.is_empty() {
            self.lines[index].ending = self.newline.clone();
        }
        self.lines.insert(
            index + 1,
            IniLine {
                text: text.to_string(),
                ending: self.newline.clone(),
            },
        );
    }

    /// Rewrite literal `\n` from the database into the document's own ending.
    fn localize(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let bytes = text.as_bytes();
        let mut index = 0;

        while index < bytes.len() {
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                out.push_str(&self.newline);
                index += 2;
            } else if bytes[index] == b'\n' {
                out.push_str(&self.newline);
                index += 1;
            } else {
                let start = index;
                while index < bytes.len() && bytes[index] != b'\n' && bytes[index] != b'\r' {
                    index += 1;
                }
                out.push_str(&text[start..index]);
            }
        }

        out
    }

    // MARK: execution

    fn execute_group(&mut self, entry: &'a Entry, ctx: &mut Ctx) {
        for command in &entry.commands {
            if ctx.error.is_some() {
                return;
            }
            self.execute_command(command, ctx);
            self.drain_queue(ctx);
        }
    }

    fn drain_queue(&mut self, ctx: &mut Ctx) {
        while ctx.error.is_none() {
            let Some(queued) = ctx.queued.pop_front() else {
                return;
            };
            self.execute_queued(queued, ctx);
        }
    }

    fn execute_command(&mut self, command: &RawCommand, ctx: &mut Ctx) {
        match command.cmd.as_str() {
            "log" => {}
            "update_hash" => self.update_hash(command, ctx),
            "add_section_if_missing" => self.add_section_if_missing(command, ctx),
            "multiply_section_if_missing" => self.multiply_section_if_missing(command, ctx),
            "add_ib_check_if_missing" => self.add_ib_check_if_missing(ctx),
            other if is_buffer_command(other) => self.report_unsupported(
                other,
                command
                    .args
                    .get("hash")
                    .and_then(|value| value.as_str())
                    .unwrap_or(&ctx.hash),
                &ctx.label,
                REASON_BUFFER,
            ),
            other if UNPORTED.contains(&other) => {
                self.report_unsupported(other, &ctx.hash, &ctx.label, REASON_UNPORTED)
            }
            other => self.report_unsupported(other, &ctx.hash, &ctx.label, REASON_UNKNOWN),
        }
    }

    fn report_unsupported(&mut self, command: &str, hash: &str, label: &str, reason: &str) {
        if reason == REASON_BUFFER {
            self.report.needs_buffer_pass = true;
        }
        let entry = UnsupportedCommand {
            command: command.to_string(),
            hash: hash.to_string(),
            label: label.to_string(),
            reason: reason.to_string(),
        };
        if !self.report.unsupported.contains(&entry) {
            self.report.unsupported.push(entry);
        }
    }

    fn execute_queued(&mut self, queued: Queued, ctx: &mut Ctx) {
        match queued {
            Queued::CaptureSection {
                content,
                position,
            } => self.capture_section(ctx, content.as_deref(), position.as_deref()),
            Queued::CreateNewSection {
                section_content,
                saved_position,
            } => self.create_new_section(ctx, &section_content, saved_position.as_deref()),
        }
    }

    /// `update_hash`: point the current hash's lines at a newer one, keeping the
    /// old value behind as a comment.
    ///
    /// An ignored update is reported and skipped *whole*: nothing is rewritten
    /// and the new hash is not queued, so fixes somebody else keys off it are
    /// not attempted against a file that does not contain it.
    fn update_hash(&mut self, command: &RawCommand, ctx: &mut Ctx) {
        let Some(new_hash) = command.args.get("new_hash").and_then(|v| v.as_str()) else {
            ctx.error = Some(format!("update_hash: {REASON_MALFORMED}"));
            return;
        };

        let old = ctx.hash.clone();
        // Upstream's data contains at least one `update_hash` cycle (Grace's
        // 89d903ba <-> d21f32ad). Rewriting into a cycle changes the file on
        // every run and never settles, so it is reported and skipped.
        if self.leads_back(&old) {
            self.report_unsupported("update_hash", &old, &ctx.label, REASON_CYCLE);
            return;
        }

        // Every uncommented `hash = <old>` line is rewritten, including one on
        // the first line of the file (upstream's `\n`-prefixed pattern cannot
        // match that one).
        let matches: Vec<usize> = (0..self.lines.len())
            .filter(|index| {
                hash_value(&self.lines[*index].text)
                    .is_some_and(|value| value.eq_ignore_ascii_case(&old))
            })
            .collect();

        let key = hash_key(&old, new_hash);

        if !matches.is_empty() {
            if self.ignored.contains(&key) {
                // Skip this hash's whole bundle: the rewrite, and the section
                // inserts and IB checks that only make sense once the file
                // carries the newer hash. All of them stay visible in the
                // report, so the scan says exactly what an apply will not do.
                ctx.ignored = true;
                self.report.updated.push(HashUpdate {
                    from: old,
                    to: new_hash.to_string(),
                    label: ctx.label.clone(),
                    key,
                    ignored: true,
                });
                return;
            }

            for index in matches.iter().rev() {
                let Some(parsed) = parse_hash_line(&self.lines[*index].text) else {
                    continue;
                };

                let original = std::mem::take(&mut self.lines[*index].text);
                let ending = self.lines[*index].ending.clone();
                let key_start = parsed.key_start;

                // `{indent}hash = <new>` on the original line, the old spelling on
                // the line below it - both with the file's own line ending.
                self.lines[*index].text = format!("{}hash = {new_hash}", &original[..key_start]);
                self.lines[*index].ending = self.newline.clone();
                self.lines.insert(
                    *index + 1,
                    IniLine {
                        text: format!("; {}", &original[key_start..]),
                        ending,
                    },
                );
            }

            self.report.touched = true;
            self.report.updated.push(HashUpdate {
                from: old,
                to: new_hash.to_string(),
                label: ctx.label.clone(),
                key,
                ignored: false,
            });
        }

        ctx.hash = new_hash.to_string();
        self.present.insert(new_hash.to_ascii_lowercase());
        ctx.queued_hashes.push(new_hash.to_string());
    }

    fn add_section_if_missing(&mut self, command: &RawCommand, ctx: &mut Ctx) {
        let equivalents = command.hashes_arg("equiv_hashes");
        let title = command.str_arg("section_title").unwrap_or_default();

        if self.any_present(&equivalents) {
            return;
        }
        let Some(equivalent) = equivalents.first().cloned() else {
            ctx.error = Some(format!("add_section_if_missing: {REASON_MALFORMED}"));
            return;
        };

        let content = self.localize(command.str_arg("section_content").unwrap_or_default());
        let section = format!(
            "{nl}[TextureOverride{title}]{nl}hash = {equivalent}{nl}{content}",
            nl = self.newline
        );

        self.schedule_addition(&equivalent, title, section, ctx);
    }

    fn multiply_section_if_missing(&mut self, command: &RawCommand, ctx: &mut Ctx) {
        let equivalents = command.hashes_arg("equiv_hashes");
        let title = command.str_arg("extra_title").unwrap_or_default();

        if self.any_present(&equivalents) {
            return;
        }
        let Some(equivalent) = equivalents.first().cloned() else {
            ctx.error = Some(format!("multiply_section_if_missing: {REASON_MALFORMED}"));
            return;
        };

        // A copy of this hash's section under the other resolution's hash.
        let section = format!(
            "{nl}[TextureOverride{title}]{nl}hash = {equivalent}{nl}{PLACEHOLDER_CONTENT}{nl}",
            nl = self.newline
        );

        self.schedule_addition(&equivalent, title, section, ctx);
    }

    fn schedule_addition(&mut self, equivalent: &str, title: &str, section: String, ctx: &mut Ctx) {
        let section_name = format!("[TextureOverride{title}]");
        let key = section_key(equivalent, &section_name);

        if ctx.ignored || self.ignored.contains(&key) {
            self.report.additions.push(SectionAddition {
                section: section_name,
                hash: equivalent.to_string(),
                label: ctx.label.clone(),
                key,
                ignored: true,
            });

            // Nothing is inserted, but the run still accounts for the section:
            // without this, a second entry proposing the same section reports it
            // a second time (and the report would promise two insertions).
            self.present.insert(equivalent.to_ascii_lowercase());
            if !ctx.ignored {
                // Only this recommendation was ignored, so the fixes that key
                // off the same hash still run.
                ctx.queued_hashes.push(equivalent.to_string());
            }
            return;
        }

        self.report.additions.push(SectionAddition {
            section: section_name,
            hash: equivalent.to_string(),
            label: ctx.label.clone(),
            key,
            ignored: false,
        });

        self.present.insert(equivalent.to_ascii_lowercase());
        ctx.queued_hashes.push(equivalent.to_string());
        ctx.queued.push_back(Queued::CaptureSection {
            content: Some(PLACEHOLDER_CONTENT.to_string()),
            position: Some(PLACEHOLDER_POSITION.to_string()),
        });
        ctx.queued.push_back(Queued::CreateNewSection {
            section_content: section,
            saved_position: Some(PLACEHOLDER_POSITION.to_string()),
        });
    }

    fn any_present(&self, hashes: &[String]) -> bool {
        hashes
            .iter()
            .any(|hash| self.present.contains(&hash.to_ascii_lowercase()))
    }

    /// True when following this hash's `update_hash` edges comes back to it.
    fn leads_back(&self, start: &str) -> bool {
        let mut current = start.to_string();

        for _ in 0..8 {
            let Some(entry) = self.db.get(&current) else {
                return false;
            };
            let Some(next) = entry.commands.iter().find_map(|command| {
                (command.cmd == "update_hash")
                    .then(|| command.args.get("new_hash").and_then(|value| value.as_str()))
                    .flatten()
            }) else {
                return false;
            };

            if next.eq_ignore_ascii_case(start) {
                return true;
            }
            current = next.to_string();
        }

        false
    }

    /// `add_ib_check_if_missing`: make sure this hash's IB sections run the
    /// skin-texture commandlist, so the mod keeps rendering after an update.
    fn add_ib_check_if_missing(&mut self, ctx: &mut Ctx) {
        if ctx.ignored {
            // The hash this check belongs to is not going to be rewritten, so
            // there is nothing for the check to keep rendering.
            return;
        }

        let sections = self.sections_with_hash(&ctx.hash);

        let mut insertions = Vec::new();
        let mut indexed = 0usize;
        let mut last_unindexed = None;

        for section in &sections {
            let body = section.header + 1..section.end;
            let has_index = body
                .clone()
                .any(|line| setting_key(&self.lines[line].text).is_some_and(|key| key == "match_first_index"));

            if !has_index {
                last_unindexed = Some(*section);
                continue;
            }

            indexed += 1;
            if body.clone().any(|line| self.runs_skin_texture(line)) {
                continue;
            }
            let line = body
                .clone()
                .find(|line| setting_key(&self.lines[*line].text).is_some_and(|key| key == "match_first_index"))
                .expect("match_first_index line was just found");
            insertions.push(line);
        }

        // Upstream only inspects the unindexed section when the hash has no
        // indexed one at all; mirror that rather than inventing behaviour.
        if indexed == 0 {
            if let Some(section) = last_unindexed {
                let body = section.header + 1..section.end;
                if !body.clone().any(|line| self.runs_skin_texture(line)) {
                    insertions.push(section.hash_line);
                }
            }
        }

        if insertions.is_empty() {
            return;
        }

        insertions.sort_unstable();
        for line in insertions.iter().rev() {
            self.insert_line_after(*line, &format!("run = {SKIN_TEXTURE_RUN}"));
        }
        self.report.touched = true;
    }

    fn runs_skin_texture(&self, line: usize) -> bool {
        setting(&self.lines[line].text).is_some_and(|(key, value)| {
            key == "run" && value.eq_ignore_ascii_case(SKIN_TEXTURE_RUN)
        })
    }

    fn capture_section(&mut self, ctx: &mut Ctx, content: Option<&str>, position: Option<&str>) {
        let Some(section) = self.sections_with_hash(&ctx.hash).first().copied() else {
            ctx.error = Some(format!(
                "no section with hash = {} in this INI",
                ctx.hash
            ));
            return;
        };

        let critical = self.critical_content(section);
        if let Some(key) = content {
            ctx.data.set(key, critical);
        }
        if let Some(key) = position {
            ctx.data.set(key, section.end.to_string());
        }
    }

    fn create_new_section(&mut self, ctx: &mut Ctx, section_content: &str, saved_position: Option<&str>) {
        let mut content = section_content.to_string();
        for (placeholder, value) in &ctx.data.entries {
            if placeholder.starts_with('_') {
                continue;
            }
            content = content.replace(placeholder.as_str(), value);
        }

        if content.contains(PLACEHOLDER_CONTENT)
            || content.contains(PLACEHOLDER_INDEXED)
            || content.contains(PLACEHOLDER_POSITION)
        {
            ctx.error = Some(REASON_SUBSTITUTION.to_string());
            return;
        }

        let index = saved_position
            .and_then(|key| ctx.data.get(key))
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(self.lines.len());

        self.insert_text(index, &content);
        self.report.touched = true;
    }
}

// MARK: line helpers

fn is_commented(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with(';') || trimmed.starts_with('#')
}

/// The value of an uncommented `hash = …` line, hex characters only (upstream's
/// `[a-f0-9]*`, which can also be empty).
fn hash_value(text: &str) -> Option<&str> {
    let parsed = parse_hash_line(text)?;
    Some(&text[parsed.value_start..parsed.value_end])
}

struct HashLine {
    /// First non-blank character of the line.
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

fn parse_hash_line(text: &str) -> Option<HashLine> {
    if is_commented(text) {
        return None;
    }

    let key_start = text.len() - text.trim_start().len();
    let equals = text[key_start..].find('=')? + key_start;
    if !text[key_start..equals].trim_end().eq_ignore_ascii_case("hash") {
        return None;
    }

    let value_start = equals
        + 1
        + (text[equals + 1..].len() - text[equals + 1..].trim_start().len());
    let value_end = value_start
        + text[value_start..]
            .chars()
            .take_while(|character| character.is_ascii_hexdigit())
            .count();

    Some(HashLine {
        key_start,
        value_start,
        value_end,
    })
}

/// `key = value` (uncommented, key trimmed); the value stops at a `;` comment,
/// like the INI parser's own setter.
fn setting(text: &str) -> Option<(&str, &str)> {
    if is_commented(text) {
        return None;
    }

    let equals = text.find('=')?;
    let key = text[..equals].trim();
    if key.is_empty() {
        return None;
    }

    let value = text[equals + 1..].trim();
    let value = value.split(';').next().unwrap_or(value).trim();
    Some((key, value))
}

fn setting_key(text: &str) -> Option<&str> {
    setting(text).map(|(key, _)| key)
}

/// A line that a captured section must keep: not a header, not `hash`, not
/// `match_first_index`.
fn is_critical(text: &str) -> bool {
    if text.trim_start().starts_with('[') {
        return false;
    }

    !matches!(setting_key(text).map(str::to_ascii_lowercase).as_deref(), Some("hash") | Some("match_first_index"))
}

/// True for a `[TextureOverride…]` / `[ShaderOverride…]` header line. Any other
/// `[…]` section (`[Key…]`, `[Resource…]`, `[Constants]`) is not a target.
fn is_override_header(text: &str) -> bool {
    if is_commented(text) {
        return false;
    }

    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return false;
    };
    let Some(end) = rest.find(']') else {
        return false;
    };
    let name = &rest[..end];

    starts_with_ignore_case(name, "TextureOverride")
        || starts_with_ignore_case(name, "ShaderOverride")
}

/// Case-insensitive prefix test that cannot split a multi-byte character (a
/// section name is arbitrary user text).
fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database_from(entries: &str) -> Database {
        Database::from_json(&format!(
            r#"{{"source":{{"repo":"","ref":"","refDate":"","gameVersion":"","license":""}},
                "characters":1,"hashCount":0,
                "commands":{{"used":["log"],"bufferPass":[]}},
                "hashCommands":{{{entries}}}}}"#
        ))
        .expect("test database")
    }

    fn scan_hashes_of(text: &str) -> Vec<String> {
        let ignored = HashSet::new();
        Runner {
            db: Database::bundled(),
            lines: split_lines(text),
            newline: "\r\n".to_string(),
            present: Default::default(),
            report: RunReport::default(),
            ignored: &ignored,
        }
        .scan_hashes()
    }

    #[test]
    fn scan_ignores_commented_hash_lines() {
        let text = "[TextureOverrideX]\r\nhash = 3c8697e8\r\n; hash = 15f933dc\r\n  # hash = baa3c836\r\n";

        let hashes = scan_hashes_of(text);

        assert_eq!(hashes, vec!["3c8697e8".to_string()]);
    }

    #[test]
    fn scan_takes_the_hex_prefix_only() {
        assert_eq!(hash_value("hash = 3c8697e8 ; note"), Some("3c8697e8"));
        assert_eq!(hash_value("\tHASH=ABCdef12"), Some("ABCdef12"));
        assert_eq!(hash_value("hash ="), Some(""));
        assert_eq!(hash_value("match_first_index = 1"), None);
        assert_eq!(hash_value("hashish = 1"), None);
        assert_eq!(hash_value(";hash = 1"), None);
    }

    #[test]
    fn override_headers_are_recognised() {
        assert!(is_override_header("[TextureOverrideFoo.IB]"));
        assert!(is_override_header("  [ShaderOverrideBar]"));
        assert!(!is_override_header("[KeyToggle]"));
        assert!(!is_override_header("; [TextureOverrideFoo]"));
        assert!(!is_override_header("[TextureOverrideFoo"));
    }

    #[test]
    fn unknown_commands_are_reported_not_swallowed() {
        let db = database_from(r#""aaaabbbb":{"character":"T","label":"L","commands":[{"cmd":"future_command","args":{}}]}"#);
        let outcome = run("[TextureOverrideX]\nhash = aaaabbbb\n", &db);

        assert!(!outcome.report.touched);
        let unsupported = &outcome.report.unsupported;
        assert_eq!(unsupported.len(), 1);
        assert_eq!(unsupported[0].command, "future_command");
        assert_eq!(unsupported[0].reason, REASON_UNKNOWN);
        assert_eq!(unsupported[0].hash, "aaaabbbb");
    }

    #[test]
    fn buffer_commands_are_reported_and_never_applied() {
        let db = database_from(
            r#""aaaabbbb":{"character":"T","label":"Texcoord","commands":[{"cmd":"zzz_13_remap_texcoord","args":{"id":"1"}}]}"#,
        );
        let text = "[TextureOverrideX]\nhash = aaaabbbb\nstride = 40\n";
        let outcome = run(text, &db);

        assert!(!outcome.report.touched, "no .buf work is done in v1");
        assert_eq!(outcome.text, text);
        assert!(outcome.report.needs_buffer_pass);
        assert_eq!(outcome.report.unsupported[0].reason, REASON_BUFFER);
    }

    #[test]
    fn an_update_cycle_is_reported_and_not_written() {
        // Grace's 1.2 and 1.1 texcoord hashes point at each other upstream.
        let db = database_from(
            r#""aaaabbbb":{"character":"T","label":"A","commands":[{"cmd":"log","args":{}},{"cmd":"update_hash","args":{"new_hash":"ccccdddd"}}]},
               "ccccdddd":{"character":"T","label":"B","commands":[{"cmd":"update_hash","args":{"new_hash":"aaaabbbb"}}]}"#,
        );
        let text = "[TextureOverrideX]\r\nhash = aaaabbbb\r\nib = a.buf\r\n";
        let outcome = run(text, &db);

        assert!(!outcome.report.touched);
        assert_eq!(outcome.text, text);
        assert_eq!(outcome.report.unsupported[0].reason, REASON_CYCLE);
        assert_eq!(outcome.report.unsupported[0].hash, "aaaabbbb");
    }

    /// Upstream's `\n`-prefixed scan cannot see a hash on the very first line.
    /// This port reads every line, so such a hash is found and rewritten too -
    /// and the section below it gives the other fixes their content.
    #[test]
    fn a_hash_on_the_first_line_is_found_and_rewritten() {
        let text = "hash = 3c8697e8\r\n\r\n[TextureOverrideSoldier11Head]\r\nhash = 3c8697e8\r\nmatch_priority = 0\r\n";

        let outcome = run(text, Database::bundled());

        assert!(outcome.report.error.is_none(), "{:?}", outcome.report.error);
        assert!(outcome.report.touched);
        assert!(
            outcome.text.starts_with("hash = 67821d9d\r\n; hash = 3c8697e8\r\n"),
            "first line not rewritten:\n{}",
            outcome.text
        );
        assert_eq!(outcome.text.matches("hash = 67821d9d").count(), 2);
        assert_eq!(outcome.report.updated.len(), 1);
        assert_eq!(outcome.report.updated[0].from, "3c8697e8");
        assert_eq!(outcome.report.updated[0].to, "67821d9d");
    }

    #[test]
    fn characters_come_from_the_fix_data() {
        let db = Database::bundled();
        let text = "[TextureOverrideX]\r\nhash = 3c8697e8\r\n";

        let outcome = run(text, db);

        assert_eq!(outcome.report.characters, vec!["Soldier11".to_string()]);
        assert_eq!(outcome.report.character(), "Soldier11");
    }

    /// The two shapes an ignored recommendation takes, and the guarantee that
    /// matters: scan and apply run the same filtered engine, so a report never
    /// promises a rewrite that an apply with the same ignore list will not do.
    #[test]
    fn ignored_recommendations_are_reported_but_never_applied() {
        let update = "[TextureOverrideX]\r\nhash = 3c8697e8\r\n";
        let update_key = hash_key("3C8697E8", "67821D9D");

        let ignored: HashSet<String> = [update_key.clone()].into();
        let outcome = run_with_ignore(update, Database::bundled(), &ignored);

        assert_eq!(outcome.text, update, "an ignored update writes nothing");
        assert!(!outcome.report.touched);
        assert_eq!(outcome.report.updated.len(), 1);
        assert!(outcome.report.updated[0].ignored);
        assert_eq!(outcome.report.updated[0].to, "67821d9d");
        assert_eq!(outcome.report.updated[0].key, update_key, "keys are lowercased");
        // The section insert bundled with that hash is reported too - visible in
        // the list, and not written either.
        assert_eq!(outcome.report.additions.len(), 1);
        assert!(outcome.report.additions[0].ignored);
        assert!(outcome.report.error.is_none(), "{:?}", outcome.report.error);

        // The same file without the ignore still rewrites and adds the section.
        let plain = run(update, Database::bundled());
        assert!(plain.report.touched);
        assert!(!plain.report.updated[0].ignored);
        assert_eq!(plain.report.updated[0].key, update_key);
        assert_eq!(plain.report.additions.len(), 1);
        assert!(!plain.report.additions[0].ignored);
        assert!(plain.text.contains(&plain.report.additions[0].section));
    }

    #[test]
    fn an_ignored_section_addition_leaves_the_hash_fix_alone() {
        let text = "[TextureOverrideSoldier11Head]\r\nhash = 3c8697e8\r\nmatch_priority = 0\r\n\r\n[TextureOverrideOther]\r\nhash = ffffffff\r\nib = data.buf\r\n";

        let plain = run(text, Database::bundled());
        assert_eq!(plain.report.additions.len(), 1, "{:?}", plain.report.additions);
        let addition = plain.report.additions[0].clone();
        assert!(!addition.ignored);
        assert!(addition.key.starts_with("section:"), "key: {}", addition.key);

        let ignored: HashSet<String> = [addition.key.clone()].into();
        let outcome = run_with_ignore(text, Database::bundled(), &ignored);

        assert!(outcome.report.touched, "the hash update still applies");
        assert!(outcome.text.contains("hash = 67821d9d"));
        assert!(!outcome.text.contains(&addition.section));
        assert!(outcome.report.updated.iter().all(|change| !change.ignored));
        assert_eq!(outcome.report.additions.len(), 1, "{:?}", outcome.report.additions);
        assert!(outcome.report.additions[0].ignored);
        assert!(outcome.report.error.is_none(), "{:?}", outcome.report.error);
    }

    /// The per-fix scope is one row, not the file: ignoring a recommendation
    /// must leave every other recommendation in the same INI working.
    #[test]
    fn ignoring_one_recommendation_leaves_the_others_in_the_file_working() {
        // Soldier11 (1.0 head diffuse) and Lucy (1.2 body texcoord) live in
        // different database entries, so neither depends on the other.
        let text = "[TextureOverrideSoldier11Head]\r\nhash = 3c8697e8\r\n\r\n[TextureOverrideLucyBody]\r\nhash = 00f11ea6\r\n";
        let ignored: HashSet<String> = [hash_key("3c8697e8", "67821d9d")].into();

        let outcome = run_with_ignore(text, Database::bundled(), &ignored);

        assert!(outcome.report.error.is_none(), "{:?}", outcome.report.error);
        assert!(outcome.report.touched);

        let soldier = outcome
            .report
            .updated
            .iter()
            .find(|change| change.from == "3c8697e8")
            .expect("the ignored row is still reported");
        assert!(soldier.ignored);

        let lucy = outcome
            .report
            .updated
            .iter()
            .find(|change| change.from == "00f11ea6")
            .expect("the other recommendation still applies");
        assert!(!lucy.ignored);
        assert_eq!(lucy.to, "f60dbb9e");

        assert!(outcome.text.contains("hash = 3c8697e8"), "the ignored hash stays");
        assert!(!outcome.text.contains("67821d9d"));
        assert!(outcome.text.contains("hash = f60dbb9e"), "the other hash is rewritten");
        assert!(outcome.text.contains("; hash = 00f11ea6"), "the old value is kept");
    }
}

