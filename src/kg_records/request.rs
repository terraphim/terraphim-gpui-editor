//! Local JSON requests for the same Markdown role editor used by GPUI.

use super::{RoleSource, Source};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Load,
    Save {
        revision: String,
        concept_id: String,
        synonyms: Vec<String>,
    },
}

#[derive(Serialize)]
struct Snapshot {
    revision: String,
    entries: Vec<Entry>,
}

#[derive(Serialize)]
struct Entry {
    term: String,
    concept_id: String,
    canonical: String,
    display: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    synonyms: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_path: Option<String>,
}

/// Handle a local request from an editor that uses the shared Markdown backend.
///
/// A save must supply the revision from a previous load or successful save.
#[must_use]
pub fn markdown_kg_request(root: &Path, request: &[u8]) -> Value {
    match execute(root, request) {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(error) => json!({ "ok": false, "error": error.to_string() }),
    }
}

fn execute(root: &Path, payload: &[u8]) -> Result<Snapshot> {
    let request = serde_json::from_slice::<Request>(payload)?;
    ensure!(root.is_dir(), "Supply a Markdown role directory");
    let RoleSource(Source::Markdown(mut source)) = RoleSource::load(root)? else {
        anyhow::bail!("Supply a Markdown role directory");
    };
    if let Request::Save {
        revision,
        concept_id,
        synonyms,
    } = request
    {
        ensure!(
            revision == source.revision(),
            "The Markdown role changed on disk. Reload it before editing its synonyms."
        );
        let mut record = source.record(concept_id.parse()?)?;
        record.synonyms = synonyms;
        source.save(&record)?;
    }

    let mut entries = Vec::new();
    for (term, concept) in &source.thesaurus() {
        let canonical = concept.value.as_str();
        let (synonyms, source_path) = if term.as_str() == canonical {
            (
                Some(source.record(concept.id)?.synonyms),
                Some(source.record_path(concept.id)?.display().to_string()),
            )
        } else {
            (None, None)
        };
        entries.push(Entry {
            term: term.to_string(),
            concept_id: concept.id.to_string(),
            canonical: canonical.to_owned(),
            display: concept.display().to_owned(),
            synonyms,
            source_path,
        });
    }
    entries.sort_by(|left, right| left.term.cmp(&right.term));
    Ok(Snapshot {
        revision: source.revision(),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::markdown_kg_request;
    use serde_json::json;
    use std::fs;

    #[test]
    fn one_shot_requests_save_and_reject_stale_role_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("copper alloy.md");
        fs::write(&source, "# Copper Alloy\n\nsynonyms:: bronze\n").unwrap();
        let initial = markdown_kg_request(directory.path(), br#"{"op":"load"}"#);
        assert_eq!(initial["ok"], true);
        let role = &initial["result"];
        let canonical = role["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["term"] == "copper alloy")
            .unwrap();
        let edit = json!({
            "op": "save",
            "revision": role["revision"],
            "concept_id": canonical["concept_id"],
            "synonyms": ["bronze", "bell metal"],
        })
        .to_string();
        let saved = markdown_kg_request(directory.path(), edit.as_bytes());
        assert_eq!(saved["ok"], true);
        assert!(fs::read_to_string(&source)
            .unwrap()
            .contains("synonyms:: bronze, bell metal"));
        assert_ne!(saved["result"]["revision"], role["revision"]);
        assert_eq!(
            markdown_kg_request(directory.path(), edit.as_bytes())["ok"],
            false
        );
    }
}
