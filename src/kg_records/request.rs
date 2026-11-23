//! Local JSON requests for the same Markdown role editor used by GPUI.

use super::{RoleSource, Source};
use crate::dependency_tree::TreeGroup;
use crate::{document, SlashCommandRegistry, TerraphimCompletionProvider};
use anyhow::{ensure, Context as _, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
use terraphim_alternatives::{byte_to_utf16, utf16_to_byte, Document};
use terraphim_automata::autocomplete::build_autocomplete_index;
use terraphim_lsp_core::KgEngine;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Load,
    ConceptTree {
        concept_id: String,
        revision: Option<String>,
    },
    Complete {
        text: String,
        offset: usize,
        revision: Option<String>,
    },
    Cycle {
        text: String,
        offset: usize,
        forward: bool,
        revision: Option<String>,
    },
    Save {
        revision: String,
        concept_id: String,
        synonyms: Vec<String>,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum Response {
    Snapshot(Snapshot),
    Tree {
        groups: Vec<TreeGroup>,
    },
    Completion {
        revision: String,
        items: Vec<Completion>,
    },
    Cycle {
        revision: String,
        text: String,
        caret: usize,
    },
}

#[derive(Serialize)]
struct Completion {
    label: String,
    detail: Option<String>,
    insert_text: String,
    start: usize,
    end: usize,
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

fn execute(root: &Path, payload: &[u8]) -> Result<Response> {
    let request = serde_json::from_slice::<Request>(payload)?;
    ensure!(root.is_dir(), "Supply a Markdown role directory");
    let RoleSource(Source::Markdown(mut source)) = RoleSource::load(root)? else {
        anyhow::bail!("Supply a Markdown role directory");
    };
    let revision = source.revision();
    match request {
        Request::Load => {}
        Request::ConceptTree {
            concept_id,
            revision: expected,
        } => {
            check_revision(expected.as_deref(), &revision)?;
            let groups = source.concept_tree(concept_id.parse()?)?;
            return Ok(Response::Tree { groups });
        }
        Request::Save {
            revision: expected,
            concept_id,
            synonyms,
        } => {
            check_revision(Some(&expected), &revision)?;
            let mut record = source.record(concept_id.parse()?)?;
            record.synonyms = synonyms;
            source.save(&record)?;
        }
        Request::Complete {
            text,
            offset,
            revision: expected,
        } => {
            check_revision(expected.as_deref(), &revision)?;
            let byte = utf16_to_byte(&text, offset).context("Invalid UTF-16 cursor offset")?;
            let index = Arc::new(build_autocomplete_index(source.thesaurus(), None)?);
            let provider =
                TerraphimCompletionProvider::with_defaults(index, SlashCommandRegistry::new());
            let mut items = Vec::new();
            if let Some((range, suggestions)) = provider.term_completions(&text, byte) {
                let start =
                    byte_to_utf16(&text, range.start).context("Invalid completion start")?;
                let end = byte_to_utf16(&text, range.end).context("Invalid completion end")?;
                items.extend(suggestions.into_iter().filter_map(|item| {
                    Some(Completion {
                        label: item.label,
                        detail: item.detail,
                        insert_text: item.insert_text?,
                        start,
                        end,
                    })
                }));
            }
            return Ok(Response::Completion { revision, items });
        }
        Request::Cycle {
            text,
            offset,
            forward,
            revision: expected,
        } => {
            check_revision(expected.as_deref(), &revision)?;
            utf16_to_byte(&text, offset).context("Invalid UTF-16 cursor offset")?;
            let graph = KgEngine::new(&source.thesaurus())?;
            let mut document = Document::new(text);
            let caret = document::cycle_kg(&mut document, &graph, offset, forward)?;
            return Ok(Response::Cycle {
                revision,
                text: document.body,
                caret,
            });
        }
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
    Ok(Response::Snapshot(Snapshot {
        revision: source.revision(),
        entries,
    }))
}

fn check_revision(expected: Option<&str>, actual: &str) -> Result<()> {
    ensure!(
        expected.is_none_or(|value| value == actual),
        "The Markdown role changed on disk. Reload it before editing its synonyms."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::markdown_kg_request;
    use serde_json::json;
    use std::fs;

    fn request(root: &std::path::Path, value: &serde_json::Value) -> serde_json::Value {
        markdown_kg_request(root, value.to_string().as_bytes())
    }

    #[test]
    fn concept_tree_resolves_canonical_headings_and_cuts_cycles() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let alpha = "---\ndepends_on:: Hidden\n---\n\n# Alpha\n\nsynonyms:: first alias\n\ndepends_on::   bETA\ndepends_on:: Absent\ndepends_on:: Duplicate\ndepends_on:: beta alias\n\n```text\ndepends_on:: Hidden\n```\n";
        for (name, text) in [
            ("alpha.md", alpha),
            (
                "different filename.md",
                "# Beta\n\nsynonyms:: beta alias\ndepends_on:: Alpha\n",
            ),
            ("hidden.md", "# Hidden\n"),
            ("duplicate-one.md", "# Duplicate\n"),
            ("duplicate-two.md", "# Duplicate\n"),
        ] {
            fs::write(root.join(name), text).unwrap();
        }

        let role = request(root, &json!({"op":"load"}));
        let entries = role["result"]["entries"].as_array().unwrap();
        let id = entries
            .iter()
            .find(|entry| entry["term"] == "first alias")
            .unwrap()["concept_id"]
            .clone();
        let tree = request(root, &json!({"op":"concept_tree", "concept_id": id}));
        assert_eq!(tree["ok"], true, "{tree}");
        let depends_on = &tree["result"]["groups"][0];
        assert_eq!(depends_on["label"], "Depends on");
        let nodes = depends_on["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["label"], "Beta");
        let cycle = &nodes[0]["children"][0];
        assert_eq!(cycle["id"], id);
        assert_eq!(cycle["cycle"], true);
        assert_eq!(cycle["children"], json!([]));
        assert_eq!(fs::read_to_string(root.join("alpha.md")).unwrap(), alpha);
    }

    #[test]
    fn concept_tree_bounds_the_role_outline() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("root concept.md"), "# Root concept\n").unwrap();
        for n in 0..140 {
            let name = format!("Child concept {n}");
            fs::write(
                root.join(format!("{name}.md")),
                format!("# {name}\n\nparent:: Root concept\n"),
            )
            .unwrap();
        }
        let response = request(root, &json!({"op":"load"}));
        let entries = response["result"]["entries"].as_array().unwrap();
        let id = &entries
            .iter()
            .find(|entry| entry["term"] == "root concept")
            .unwrap()["concept_id"];
        let tree = request(root, &json!({"op":"concept_tree", "concept_id":id}));
        let group = &tree["result"]["groups"][0];
        assert_eq!(group["label"], "Subconcepts");
        assert_eq!(group["truncated"], true);
        assert_eq!(group["nodes"].as_array().unwrap().len(), 128);
    }

    #[test]
    fn term_requests_share_gpui_ranges_and_kg_cycling() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("eraser.md"), "# Eraser\n\nsynonyms:: rubber\n").unwrap();
        let role = request(root, &json!({"op":"load"}));
        let revision = &role["result"]["revision"];
        let complete = request(
            root,
            &json!({
                "op":"complete", "text":"🦀 A rubbxx", "offset":9, "revision":revision
            }),
        );
        assert_eq!(complete["ok"], true);
        let first = &complete["result"]["items"][0];
        assert_eq!(first["label"], "rubber");
        assert_eq!(first["insert_text"], "rubber");
        assert_eq!(first["start"], 5);
        assert_eq!(first["end"], 11);
        assert_eq!(complete["result"]["revision"], *revision);

        let cycled = request(
            root,
            &json!({
                "op":"cycle", "text":"🦀 An ERASER.", "offset":8, "forward":true, "revision":revision
            }),
        );
        assert_eq!(cycled["ok"], true);
        assert_eq!(cycled["result"]["text"], "🦀 A RUBBER.");
        assert_eq!(cycled["result"]["caret"], 5);
        let back = request(
            root,
            &json!({
                "op":"cycle", "text":cycled["result"]["text"], "offset":5,
                "forward":false, "revision":revision
            }),
        );
        assert_eq!(back["result"]["text"], "🦀 An ERASER.");

        let invalid = request(root, &json!({"op":"complete", "text":"🦀", "offset":1}));
        assert_eq!(invalid["ok"], false);
        let stale = request(
            root,
            &json!({
                "op":"cycle", "text":"eraser", "offset":1, "forward":true, "revision":"stale"
            }),
        );
        assert_eq!(stale["ok"], false);
    }

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
