//! Minimal YAML frontmatter for `SKILL.md`: enough to read `name` and
//! `description` and to re-emit the document with `name` rewritten.

const REQUIRED_KEYS: [&str; 2] = ["name", "description"];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrontmatterError {
    #[error("SKILL.md does not start with a `---` frontmatter fence")]
    MissingFrontmatter,
    #[error("SKILL.md frontmatter is not terminated by `---`")]
    UnterminatedFrontmatter,
    #[error("SKILL.md frontmatter has no `{0}`")]
    MissingKey(&'static str),
    #[error("SKILL.md frontmatter has an empty `{0}`")]
    EmptyKey(&'static str),
    #[error("SKILL.md frontmatter line is not `key: value`: {0}")]
    NotAMapping(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    entries: Vec<(String, String)>,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Replaces `key` in place, or appends it when absent.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value.into(),
            None => self.entries.push((key.to_string(), value.into())),
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::from("---\n");
        for (key, value) in &self.entries {
            out.push_str(key);
            out.push_str(": ");
            out.push_str(&quote(value));
            out.push('\n');
        }
        out.push_str("---\n");
        out
    }
}

/// A parsed `SKILL.md`: validated frontmatter plus the body verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDoc {
    pub frontmatter: Frontmatter,
    pub body: String,
}

impl SkillDoc {
    pub fn parse(text: &str) -> Result<Self, FrontmatterError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let after_fence = text
            .strip_prefix("---\n")
            .or_else(|| text.strip_prefix("---\r\n"))
            .ok_or(FrontmatterError::MissingFrontmatter)?;
        let (block, body) = split_at_closing_fence(after_fence)?;
        let frontmatter = parse_block(block)?;

        for key in REQUIRED_KEYS {
            match frontmatter.get(key) {
                None => return Err(FrontmatterError::MissingKey(key)),
                Some(value) if value.trim().is_empty() => {
                    return Err(FrontmatterError::EmptyKey(key));
                }
                Some(_) => {}
            }
        }

        Ok(SkillDoc { frontmatter, body: body.to_string() })
    }

    pub fn name(&self) -> &str {
        self.frontmatter.get("name").expect("validated at parse")
    }

    pub fn render(&self) -> String {
        let mut out = self.frontmatter.render();
        out.push_str(&self.body);
        out
    }
}

fn split_at_closing_fence(text: &str) -> Result<(&str, &str), FrontmatterError> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Ok((&text[..offset], &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(FrontmatterError::UnterminatedFrontmatter)
}

fn parse_block(block: &str) -> Result<Frontmatter, FrontmatterError> {
    let lines: Vec<&str> = block.lines().collect();
    let mut frontmatter = Frontmatter::default();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }

        let (key, rest) = line
            .split_once(':')
            .ok_or_else(|| FrontmatterError::NotAMapping(line.to_string()))?;
        if key.is_empty() || key.starts_with(char::is_whitespace) {
            return Err(FrontmatterError::NotAMapping(line.to_string()));
        }
        let rest = rest.trim();

        let value = if let Some(style) = block_scalar_style(rest) {
            let (text, consumed) = read_block_scalar(&lines[index..], style);
            index += consumed;
            text
        } else {
            let (text, consumed) = read_plain_or_quoted(rest, &lines[index..]);
            index += consumed;
            text
        };

        frontmatter.set(key.trim(), value);
    }

    Ok(frontmatter)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockStyle {
    Literal,
    Folded,
}

fn block_scalar_style(rest: &str) -> Option<BlockStyle> {
    match rest.chars().next()? {
        '|' => Some(BlockStyle::Literal),
        '>' => Some(BlockStyle::Folded),
        _ => None,
    }
}

fn read_block_scalar(lines: &[&str], style: BlockStyle) -> (String, usize) {
    let mut collected: Vec<String> = Vec::new();
    let mut consumed = 0;
    let mut indent: Option<usize> = None;

    for line in lines {
        let blank = line.trim().is_empty();
        let line_indent = line.len() - line.trim_start().len();
        if !blank && indent.is_some_and(|expected| line_indent < expected) {
            break;
        }
        if !blank && indent.is_none() {
            if line_indent == 0 {
                break;
            }
            indent = Some(line_indent);
        }
        consumed += 1;
        let stripped = indent.map_or("", |width| line.get(width..).unwrap_or(""));
        collected.push(stripped.to_string());
    }

    while collected.last().is_some_and(|line| line.trim().is_empty()) {
        collected.pop();
    }

    let joined = match style {
        BlockStyle::Literal => collected.join("\n"),
        BlockStyle::Folded => fold(&collected),
    };
    (joined, consumed)
}

fn fold(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        if line.trim().is_empty() {
            out.push('\n');
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            out.push_str(line.trim_end());
        }
    }
    out
}

/// Reads a plain or quoted scalar, absorbing more-indented continuation lines.
fn read_plain_or_quoted(rest: &str, following: &[&str]) -> (String, usize) {
    let mut text = rest.to_string();
    let mut consumed = 0;

    for line in following {
        let is_continuation = !line.trim().is_empty()
            && line.starts_with(char::is_whitespace)
            && !looks_like_mapping(line);
        if !is_continuation {
            break;
        }
        consumed += 1;
        text.push(' ');
        text.push_str(line.trim());
    }

    (unquote(text.trim()), consumed)
}

fn looks_like_mapping(line: &str) -> bool {
    line.split_once(':').is_some_and(|(key, _)| !key.trim().is_empty() && !key.contains(' '))
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        return unescape_double_quoted(&value[1..value.len() - 1]);
    }
    if bytes.len() >= 2 && bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'' {
        return value[1..value.len() - 1].replace("''", "'");
    }
    value.to_string()
}

fn unescape_double_quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Emits a value that re-parses identically, quoting only when necessary.
fn quote(value: &str) -> String {
    let needs_quoting = value.is_empty()
        || value.trim() != value
        || value.contains(['\n', '\r', '\t', '"'])
        || value.contains(": ")
        || value.contains(" #")
        || value.ends_with(':')
        || value.starts_with(['|', '>', '&', '*', '!', '%', '@', '`', '\'', '[', '{', '-', '?', '#']);
    if !needs_quoting {
        return value.to_string();
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> SkillDoc {
        SkillDoc::parse(text).unwrap()
    }

    #[test]
    fn reads_plain_scalars_and_body() {
        let doc = parse("---\nname: handoff\ndescription: Hands off work\n---\n# Handoff\n\nbody\n");
        assert_eq!(doc.name(), "handoff");
        assert_eq!(doc.frontmatter.get("description"), Some("Hands off work"));
        assert_eq!(doc.body, "# Handoff\n\nbody\n");
    }

    #[test]
    fn unquotes_double_and_single_quoted_scalars() {
        let doc = parse("---\nname: \"handoff\"\ndescription: 'it''s useful'\n---\n");
        assert_eq!(doc.name(), "handoff");
        assert_eq!(doc.frontmatter.get("description"), Some("it's useful"));
    }

    #[test]
    fn keeps_colons_inside_unquoted_descriptions() {
        let doc = parse("---\nname: tdd\ndescription: Use when: writing tests\n---\n");
        assert_eq!(doc.frontmatter.get("description"), Some("Use when: writing tests"));
    }

    #[test]
    fn joins_folded_block_scalars() {
        let doc = parse("---\nname: tdd\ndescription: >-\n  first line\n  second line\nversion: 2\n---\n");
        assert_eq!(doc.frontmatter.get("description"), Some("first line second line"));
        assert_eq!(doc.frontmatter.get("version"), Some("2"));
    }

    #[test]
    fn keeps_newlines_in_literal_block_scalars() {
        let doc = parse("---\nname: tdd\ndescription: |\n  first\n  second\n---\n");
        assert_eq!(doc.frontmatter.get("description"), Some("first\nsecond"));
    }

    #[test]
    fn joins_indented_continuation_lines() {
        let doc = parse("---\nname: tdd\ndescription: first\n  second\n---\n");
        assert_eq!(doc.frontmatter.get("description"), Some("first second"));
    }

    #[test]
    fn rejects_documents_without_frontmatter() {
        assert_eq!(SkillDoc::parse("# no frontmatter\n"), Err(FrontmatterError::MissingFrontmatter));
    }

    #[test]
    fn rejects_unterminated_frontmatter() {
        assert_eq!(
            SkillDoc::parse("---\nname: x\ndescription: y\n"),
            Err(FrontmatterError::UnterminatedFrontmatter)
        );
    }

    #[test]
    fn rejects_missing_and_empty_required_keys() {
        assert_eq!(SkillDoc::parse("---\nname: x\n---\n"), Err(FrontmatterError::MissingKey("description")));
        assert_eq!(SkillDoc::parse("---\ndescription: y\n---\n"), Err(FrontmatterError::MissingKey("name")));
        assert_eq!(
            SkillDoc::parse("---\nname:\ndescription: y\n---\n"),
            Err(FrontmatterError::EmptyKey("name"))
        );
    }

    #[test]
    fn set_replaces_in_place_and_appends_new_keys() {
        let mut doc = parse("---\nname: handoff\ndescription: d\n---\nbody\n");
        doc.frontmatter.set("name", "mp:handoff");
        doc.frontmatter.set("source_alias", "mp");
        assert_eq!(
            doc.render(),
            "---\nname: mp:handoff\ndescription: d\nsource_alias: mp\n---\nbody\n"
        );
    }

    #[test]
    fn rendered_values_reparse_identically() {
        let awkward = "Use when: \"quoted\", multi\nline\tand # hash";
        let mut doc = parse("---\nname: x\ndescription: d\n---\n");
        doc.frontmatter.set("description", awkward);
        let reparsed = parse(&doc.render());
        assert_eq!(reparsed.frontmatter.get("description"), Some(awkward));
    }

    #[test]
    fn round_trips_extra_keys() {
        let doc = parse("---\nname: x\ndescription: d\nallowed-tools: Read, Bash\n---\nbody\n");
        assert_eq!(doc.frontmatter.get("allowed-tools"), Some("Read, Bash"));
        assert_eq!(parse(&doc.render()).frontmatter.get("allowed-tools"), Some("Read, Bash"));
    }
}
