//! Minimal YAML frontmatter for `SKILL.md`: enough to read `name` and
//! `description` and to inject provenance under `metadata`. Every other line
//! is re-emitted byte for byte, including comments and shapes this parser does
//! not model (ADR-0013).

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

/// A frontmatter value. `Opaque` is a shape this parser does not model: it is
/// preserved as its original lines and never rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Scalar(String),
    Map(Vec<(String, Item)>),
    Opaque,
}

/// A value inside a map. `Raw` is a value kept but not interpreted: the text
/// after its `key:` plus any more-indented lines, with the map's indent removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Scalar(String),
    Raw { inline: String, lines: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    key: String,
    value: Value,
    /// The lines this entry was parsed from; emptied once it is rewritten.
    lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    /// A comment or a blank line.
    Other(String),
    Entry(Entry),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    nodes: Vec<Node>,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        match self.value(key)? {
            Value::Scalar(scalar) => Some(scalar.as_str()),
            _ => None,
        }
    }

    /// Reads `key` from the map at `section`.
    pub fn get_in(&self, section: &str, key: &str) -> Option<&str> {
        match self.value(section)? {
            Value::Map(map) => match map.iter().find(|(k, _)| k == key)? {
                (_, Item::Scalar(scalar)) => Some(scalar.as_str()),
                (_, Item::Raw { .. }) => None,
            },
            _ => None,
        }
    }

    /// Whether the map at `section` defines `key`, whatever its value's shape.
    pub fn has_in(&self, section: &str, key: &str) -> bool {
        matches!(self.value(section), Some(Value::Map(map)) if map.iter().any(|(k, _)| k == key))
    }

    /// Whether `section` can take injected keys: absent, or a plain map.
    pub fn is_extensible_map(&self, section: &str) -> bool {
        matches!(self.value(section), None | Some(Value::Map(_)))
    }

    /// Replaces `key` in place, or appends it when absent. A replaced entry is
    /// regenerated rather than re-emitted verbatim.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.put(key, Value::Scalar(value.into()));
    }

    /// Replaces `key` inside the map at `section`, creating the map when
    /// absent. A `section` that is not a map is replaced outright; callers
    /// reject those skills before deploying them.
    pub fn set_in(&mut self, section: &str, key: &str, value: impl Into<String>) {
        let mut map = match self.value(section) {
            Some(Value::Map(map)) => map.clone(),
            _ => Vec::new(),
        };
        match map.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = Item::Scalar(value.into()),
            None => map.push((key.to_string(), Item::Scalar(value.into()))),
        }
        self.put(section, Value::Map(map));
    }

    pub fn render(&self) -> String {
        let mut out = String::from("---\n");
        for node in &self.nodes {
            match node {
                Node::Other(line) => {
                    out.push_str(line);
                    out.push('\n');
                }
                Node::Entry(entry) if entry.lines.is_empty() => {
                    out.push_str(&render_entry(&entry.key, &entry.value));
                }
                Node::Entry(entry) => {
                    for line in &entry.lines {
                        out.push_str(line);
                        out.push('\n');
                    }
                }
            }
        }
        out.push_str("---\n");
        out
    }

    fn value(&self, key: &str) -> Option<&Value> {
        self.entries()
            .find(|entry| entry.key == key)
            .map(|entry| &entry.value)
    }

    fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.nodes.iter().filter_map(|node| match node {
            Node::Entry(entry) => Some(entry),
            Node::Other(_) => None,
        })
    }

    fn put(&mut self, key: &str, value: Value) {
        let existing = self.nodes.iter_mut().find_map(|node| match node {
            Node::Entry(entry) if entry.key == key => Some(entry),
            _ => None,
        });
        match existing {
            Some(entry) => {
                entry.value = value;
                entry.lines.clear();
            }
            None => self.nodes.push(Node::Entry(Entry {
                key: key.to_string(),
                value,
                lines: Vec::new(),
            })),
        }
    }
}

fn render_entry(key: &str, value: &Value) -> String {
    match value {
        Value::Scalar(scalar) => format!("{key}: {}\n", quote(scalar)),
        Value::Map(map) => {
            let mut out = format!("{key}:\n");
            for (nested, item) in map {
                match item {
                    Item::Scalar(scalar) => {
                        out.push_str(&format!("  {nested}: {}\n", quote(scalar)));
                    }
                    Item::Raw { inline, lines } => {
                        out.push_str(&format!("  {nested}:"));
                        if !inline.is_empty() {
                            out.push(' ');
                            out.push_str(inline);
                        }
                        out.push('\n');
                        for line in lines {
                            out.push_str(&format!("  {line}\n"));
                        }
                    }
                }
            }
            out
        }
        // Only reachable for an opaque value that was never parsed from lines,
        // which the parser never produces.
        Value::Opaque => format!("{key}:\n"),
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

        Ok(SkillDoc {
            frontmatter,
            body: body.to_string(),
        })
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
        let start = index;
        let line = lines[index];
        index += 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            frontmatter.nodes.push(Node::Other(line.to_string()));
            continue;
        }

        let (key, rest) = line
            .split_once(':')
            .ok_or_else(|| FrontmatterError::NotAMapping(line.to_string()))?;
        if key.is_empty() || key.starts_with(char::is_whitespace) {
            return Err(FrontmatterError::NotAMapping(line.to_string()));
        }
        let rest = rest.trim();

        let value = if rest.is_empty() && starts_indented_block(&lines[index..]) {
            let (value, consumed) = read_indented_block(&lines[index..]);
            index += consumed;
            value
        } else if let Some(style) = block_scalar_style(rest) {
            let (text, consumed) = read_block_scalar(&lines[index..], style);
            index += consumed;
            Value::Scalar(text)
        } else if rest.starts_with('{') {
            // Unquoting leaves text that starts with `{` untouched.
            let (text, consumed) = read_plain_or_quoted(rest, &lines[index..]);
            index += consumed;
            parse_flow_map(&text).map_or(Value::Opaque, Value::Map)
        } else {
            let (text, consumed) = read_plain_or_quoted(rest, &lines[index..]);
            index += consumed;
            Value::Scalar(text)
        };

        frontmatter.nodes.push(Node::Entry(Entry {
            key: key.trim().to_string(),
            value,
            lines: lines[start..index].iter().map(|l| l.to_string()).collect(),
        }));
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

fn starts_indented_block(lines: &[&str]) -> bool {
    lines
        .first()
        .is_some_and(|line| !line.trim().is_empty() && line.starts_with(char::is_whitespace))
}

/// Reads the indented block under a bare `key:`. A run of `key: value` lines
/// at one indent is a map; a value it does not model — nested lines, a flow
/// collection, a block scalar — is kept raw. Anything else, such as a
/// sequence, is opaque and survives only as its original lines.
fn read_indented_block(lines: &[&str]) -> (Value, usize) {
    let indent = indent_of(lines[0]);
    let mut entries: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut modelled = true;
    let mut consumed = 0;

    for line in lines {
        if line.trim().is_empty() || indent_of(line) < indent {
            break;
        }
        consumed += 1;
        if indent_of(line) > indent {
            match entries.last_mut() {
                Some((_, _, nested)) => nested.push(line[indent..].to_string()),
                None => modelled = false,
            }
            continue;
        }
        match block_entry(line) {
            Some((key, rest)) => entries.push((key, rest, Vec::new())),
            None => modelled = false,
        }
    }

    if !modelled {
        return (Value::Opaque, consumed);
    }
    let map = entries
        .into_iter()
        .map(|(key, rest, nested)| {
            let item = if nested.is_empty() && is_plain_or_quoted(&rest) {
                Item::Scalar(unquote(&rest))
            } else {
                Item::Raw {
                    inline: rest,
                    lines: nested,
                }
            };
            (key, item)
        })
        .collect();
    (Value::Map(map), consumed)
}

/// A `key: rest` line inside a block map, with `rest` verbatim.
fn block_entry(line: &str) -> Option<(String, String)> {
    let (key, rest) = line.trim().split_once(':')?;
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    Some((key.to_string(), rest.trim().to_string()))
}

/// Whether `value` is a scalar this parser reads, rather than a flow
/// collection, block scalar, anchor, alias, tag, or null.
fn is_plain_or_quoted(value: &str) -> bool {
    !value.is_empty() && !value.starts_with(['{', '[', '|', '>', '&', '*', '!'])
}

/// Reads a one-level flow map, `{k: v, ...}`. A value that is itself a
/// collection is kept raw. `None` for anything that is not a well-formed map.
fn parse_flow_map(text: &str) -> Option<Vec<(String, Item)>> {
    let inner = text.trim().strip_prefix('{')?.strip_suffix('}')?;
    let parts = split_flow(inner, ',')?;
    let mut map = Vec::new();
    for (position, part) in parts.iter().enumerate() {
        let part = part.trim();
        if part.is_empty() {
            // `{}` and a trailing comma are the only empty parts allowed.
            if position + 1 == parts.len() {
                continue;
            }
            return None;
        }
        let mut halves = split_flow(part, ':')?.into_iter();
        let key = unquote(halves.next()?.trim());
        let value: Vec<&str> = halves.collect();
        if key.is_empty() || value.is_empty() {
            return None;
        }
        let value = value.join(":").trim().to_string();
        let item = if is_plain_or_quoted(&value) {
            Item::Scalar(unquote(&value))
        } else {
            Item::Raw {
                inline: value,
                lines: Vec::new(),
            }
        };
        map.push((key, item));
    }
    Some(map)
}

/// Splits `text` at each `separator` outside quotes and brackets. `None` when
/// the brackets or quotes do not balance.
fn split_flow(text: &str, separator: char) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0;
    for (at, c) in text.char_indices() {
        match quote {
            Some('"') if escaped => escaped = false,
            Some('"') if c == '\\' => escaped = true,
            Some(open) if c == open => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '{' | '[' => depth += 1,
                '}' | ']' => depth = depth.checked_sub(1)?,
                c if c == separator && depth == 0 => {
                    parts.push(&text[start..at]);
                    start = at + c.len_utf8();
                }
                _ => {}
            },
        }
    }
    if depth != 0 || quote.is_some() {
        return None;
    }
    parts.push(&text[start..]);
    Some(parts)
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
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
    line.split_once(':')
        .is_some_and(|(key, _)| !key.trim().is_empty() && !key.contains(' '))
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
        || value.starts_with([
            '|', '>', '&', '*', '!', '%', '@', '`', '\'', '[', '{', '-', '?', '#',
        ]);
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
        let doc =
            parse("---\nname: handoff\ndescription: Hands off work\n---\n# Handoff\n\nbody\n");
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
        assert_eq!(
            doc.frontmatter.get("description"),
            Some("Use when: writing tests")
        );
    }

    #[test]
    fn joins_folded_block_scalars() {
        let doc = parse(
            "---\nname: tdd\ndescription: >-\n  first line\n  second line\nversion: 2\n---\n",
        );
        assert_eq!(
            doc.frontmatter.get("description"),
            Some("first line second line")
        );
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
        assert_eq!(
            SkillDoc::parse("# no frontmatter\n"),
            Err(FrontmatterError::MissingFrontmatter)
        );
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
        assert_eq!(
            SkillDoc::parse("---\nname: x\n---\n"),
            Err(FrontmatterError::MissingKey("description"))
        );
        assert_eq!(
            SkillDoc::parse("---\ndescription: y\n---\n"),
            Err(FrontmatterError::MissingKey("name"))
        );
        assert_eq!(
            SkillDoc::parse("---\nname:\ndescription: y\n---\n"),
            Err(FrontmatterError::EmptyKey("name"))
        );
    }

    #[test]
    fn rewrites_only_the_keys_that_were_set() {
        let text = "---\n# who wrote this\nname: handoff\ndescription: >-\n  first line\n  second line\n\nallowed-tools: Read Bash\n---\nbody\n";
        let mut doc = parse(text);
        doc.frontmatter.set("name", "mp-handoff");

        assert_eq!(
            doc.render(),
            "---\n# who wrote this\nname: mp-handoff\ndescription: >-\n  first line\n  second line\n\nallowed-tools: Read Bash\n---\nbody\n"
        );
    }

    #[test]
    fn round_trips_an_untouched_document_byte_for_byte() {
        let text = "---\nname: x\n# a comment\ndescription: d\nmetadata:\n  author: matt\nallowed-tools:\n  - Read\n  - Bash\n---\nbody\n";
        assert_eq!(parse(text).render(), text);
    }

    #[test]
    fn reads_a_metadata_map_without_swallowing_later_keys() {
        let doc = parse(
            "---\nname: x\ndescription: d\nmetadata:\n  source: mp\n  digest: \"sha256:ab\"\nlicense: MIT\n---\nbody\n",
        );
        assert_eq!(doc.frontmatter.get_in("metadata", "source"), Some("mp"));
        assert_eq!(
            doc.frontmatter.get_in("metadata", "digest"),
            Some("sha256:ab")
        );
        assert_eq!(doc.frontmatter.get("license"), Some("MIT"));
        assert!(doc.frontmatter.is_extensible_map("metadata"));
        assert_eq!(doc.body, "body\n");
    }

    #[test]
    fn extends_an_existing_metadata_map_and_keeps_its_keys() {
        let mut doc = parse("---\nname: x\ndescription: d\nmetadata:\n  author: matt\n---\nbody\n");
        doc.frontmatter.set_in("metadata", "sm-source-id", "mp");

        assert_eq!(
            doc.render(),
            "---\nname: x\ndescription: d\nmetadata:\n  author: matt\n  sm-source-id: mp\n---\nbody\n"
        );
    }

    #[test]
    fn appends_a_metadata_map_when_absent() {
        let mut doc = parse("---\nname: x\ndescription: d\n---\nbody\n");
        doc.frontmatter.set_in("metadata", "sm-source-id", "mp");
        doc.frontmatter.set_in("metadata", "sm-source-id", "me");
        doc.frontmatter.set_in("metadata", "sm-digest", "sha256:ab");

        assert_eq!(
            doc.render(),
            "---\nname: x\ndescription: d\nmetadata:\n  sm-source-id: me\n  sm-digest: sha256:ab\n---\nbody\n"
        );
    }

    #[test]
    fn a_nested_metadata_map_is_extensible_and_keeps_its_nesting() {
        let nested = "---\nname: x\ndescription: d\nmetadata:\n    nested:\n      deeper: value\n    tools: [a, b]\n    author: matt\n---\nbody\n";
        let mut doc = parse(nested);
        assert!(doc.frontmatter.is_extensible_map("metadata"));
        assert_eq!(doc.render(), nested);
        assert_eq!(doc.frontmatter.get_in("metadata", "author"), Some("matt"));
        assert_eq!(doc.frontmatter.get_in("metadata", "nested"), None);
        assert!(doc.frontmatter.has_in("metadata", "nested"));

        doc.frontmatter.set_in("metadata", "sm-source-id", "mp");

        assert_eq!(
            doc.render(),
            "---\nname: x\ndescription: d\nmetadata:\n  nested:\n    deeper: value\n  tools: [a, b]\n  author: matt\n  sm-source-id: mp\n---\nbody\n"
        );
        let reparsed = parse(&doc.render());
        assert_eq!(
            reparsed.frontmatter.get_in("metadata", "sm-source-id"),
            Some("mp")
        );
    }

    #[test]
    fn an_inline_metadata_map_is_extensible_and_keeps_its_values() {
        let inline = "---\nname: x\ndescription: d\nmetadata: {\"openclaw\": {\"requires\": {\"env\": [\"KEY\"]}, \"primaryEnv\": \"KEY\"}, \"author\": 'matt, jr', 'sm-tags': rust}\n---\nbody\n";
        let mut doc = parse(inline);
        assert!(doc.frontmatter.is_extensible_map("metadata"));
        assert_eq!(doc.render(), inline);
        assert_eq!(
            doc.frontmatter.get_in("metadata", "author"),
            Some("matt, jr")
        );
        assert_eq!(doc.frontmatter.get_in("metadata", "sm-tags"), Some("rust"));
        assert!(doc.frontmatter.has_in("metadata", "openclaw"));

        doc.frontmatter.set_in("metadata", "sm-source-id", "el");

        assert_eq!(
            doc.render(),
            "---\nname: x\ndescription: d\nmetadata:\n  openclaw: {\"requires\": {\"env\": [\"KEY\"]}, \"primaryEnv\": \"KEY\"}\n  author: matt, jr\n  sm-tags: rust\n  sm-source-id: el\n---\nbody\n"
        );
        let reparsed = parse(&doc.render());
        assert_eq!(
            reparsed.frontmatter.get_in("metadata", "sm-source-id"),
            Some("el")
        );
        assert_eq!(
            reparsed.frontmatter.get_in("metadata", "author"),
            Some("matt, jr")
        );
    }

    #[test]
    fn an_empty_inline_metadata_map_is_extensible() {
        let doc = parse("---\nname: x\ndescription: d\nmetadata: {}\n---\nbody\n");
        assert!(doc.frontmatter.is_extensible_map("metadata"));
    }

    #[test]
    fn a_metadata_value_that_is_not_a_map_is_not_extensible() {
        for metadata in [
            "metadata: [a, b]",
            "metadata: {\"a\": 1",
            "metadata:\n  - a",
            "metadata: text",
        ] {
            let doc = parse(&format!(
                "---\nname: x\ndescription: d\n{metadata}\n---\nbody\n"
            ));
            assert!(!doc.frontmatter.is_extensible_map("metadata"), "{metadata}");
        }
    }

    #[test]
    fn a_sequence_block_is_opaque_and_survives_verbatim() {
        let sequence =
            "---\nname: x\ndescription: d\nallowed-tools:\n  - Read\n  - Bash\n---\nbody\n";
        let doc = parse(sequence);
        assert!(doc.frontmatter.is_extensible_map("metadata"));
        assert!(!doc.frontmatter.is_extensible_map("allowed-tools"));
        assert_eq!(doc.render(), sequence);
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
        assert_eq!(
            parse(&doc.render()).frontmatter.get("allowed-tools"),
            Some("Read, Bash")
        );
    }
}
