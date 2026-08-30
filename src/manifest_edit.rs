//! Format-preserving edits to `skills` selections in the manifest.

use crate::error::{Error, Result};
use toml_edit::{Array, DocumentMut, Item, Value};

/// What an edit did to the manifest text.
#[derive(Debug, PartialEq, Eq)]
pub enum Edit {
    Changed(String),
    /// The selection was already in the state the caller asked for.
    Unchanged,
}

pub fn selects_all(text: &str, alias: &str) -> Result<bool> {
    Ok(skills_array(&parse(text)?, alias).is_none())
}

pub fn add_skill(text: &str, alias: &str, skill: &str) -> Result<Edit> {
    let mut document = parse(text)?;
    let array = skills_array_mut(&mut document, alias)?;
    if contains(array, skill) {
        return Ok(Edit::Unchanged);
    }
    array.push(skill);
    Ok(Edit::Changed(document.to_string()))
}

pub fn remove_skill(text: &str, alias: &str, skill: &str) -> Result<Edit> {
    let mut document = parse(text)?;
    let array = skills_array_mut(&mut document, alias)?;
    let Some(index) = position(array, skill) else {
        return Ok(Edit::Unchanged);
    };
    array.remove(index);
    tidy_after_removal(array, index);
    Ok(Edit::Changed(document.to_string()))
}

/// Removing the first element would otherwise leave its separating space behind.
fn tidy_after_removal(array: &mut Array, removed: usize) {
    let multiline = array.to_string().contains('\n');
    if removed != 0 || multiline {
        return;
    }
    if let Some(first) = array.get_mut(0) {
        first.decor_mut().set_prefix("");
    }
}

fn parse(text: &str) -> Result<DocumentMut> {
    text.parse::<DocumentMut>()
        .map_err(|error| Error::Manifest(error.to_string()))
}

fn skills_array<'a>(document: &'a DocumentMut, alias: &str) -> Option<&'a Array> {
    document
        .get("sources")?
        .get(alias)?
        .get("skills")?
        .as_array()
}

fn skills_array_mut<'a>(document: &'a mut DocumentMut, alias: &str) -> Result<&'a mut Array> {
    let source = document
        .get_mut("sources")
        .and_then(Item::as_table_like_mut)
        .and_then(|sources| sources.get_mut(alias))
        .ok_or_else(|| Error::Manifest(format!("no source `{alias}` in the manifest")))?;

    let skills = source
        .as_table_like_mut()
        .and_then(|table| table.get_mut("skills"))
        .ok_or_else(|| {
            Error::Manifest(format!(
                "source `{alias}` omits `skills` and therefore already selects every skill"
            ))
        })?;

    match skills {
        Item::Value(Value::Array(array)) => Ok(array),
        _ => Err(Error::Manifest(format!(
            "source `{alias}` has a `skills` that is not an array"
        ))),
    }
}

fn contains(array: &Array, skill: &str) -> bool {
    position(array, skill).is_some()
}

fn position(array: &Array, skill: &str) -> Option<usize> {
    array.iter().position(|value| value.as_str() == Some(skill))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"version = 1

# third-party skills
[sources.mp]
github = "mattpocock/skills"   # pinned upstream
ref = "main"
skills = ["handoff", "tdd"]

[sources.me]
path = "~/etc/ai/skills"
"#;

    fn changed(edit: Edit) -> String {
        match edit {
            Edit::Changed(text) => text,
            Edit::Unchanged => panic!("expected a change"),
        }
    }

    #[test]
    fn adding_preserves_comments_and_layout() {
        let text = changed(add_skill(MANIFEST, "mp", "grilling").unwrap());
        assert!(text.contains("# third-party skills"));
        assert!(text.contains("# pinned upstream"));
        assert!(text.contains(r#"skills = ["handoff", "tdd", "grilling"]"#));
        assert!(text.contains(r#"path = "~/etc/ai/skills""#));
    }

    #[test]
    fn removing_preserves_comments_and_layout() {
        let text = changed(remove_skill(MANIFEST, "mp", "handoff").unwrap());
        assert!(text.contains("# third-party skills"));
        assert!(text.contains(r#"skills = ["tdd"]"#));
    }

    #[test]
    fn adding_an_existing_selection_changes_nothing() {
        assert_eq!(add_skill(MANIFEST, "mp", "tdd").unwrap(), Edit::Unchanged);
    }

    #[test]
    fn removing_an_absent_selection_changes_nothing() {
        assert_eq!(
            remove_skill(MANIFEST, "mp", "absent").unwrap(),
            Edit::Unchanged
        );
    }

    #[test]
    fn a_source_without_skills_selects_everything() {
        assert!(selects_all(MANIFEST, "me").unwrap());
        assert!(!selects_all(MANIFEST, "mp").unwrap());
        assert!(
            add_skill(MANIFEST, "me", "x")
                .unwrap_err()
                .to_string()
                .contains("every skill")
        );
        assert!(
            remove_skill(MANIFEST, "me", "x")
                .unwrap_err()
                .to_string()
                .contains("every skill")
        );
    }

    #[test]
    fn an_unknown_alias_is_an_error() {
        assert!(
            add_skill(MANIFEST, "nope", "x")
                .unwrap_err()
                .to_string()
                .contains("no source")
        );
    }

    #[test]
    fn edits_round_trip_through_the_manifest_parser() {
        let text = changed(add_skill(MANIFEST, "mp", "grilling").unwrap());
        let manifest = crate::manifest::Manifest::parse(&text).unwrap();
        assert!(manifest.sources["mp"].selects("grilling"));
    }
}
