//! File-backed knowledge graph concepts for editable editor roles.

use anyhow::{bail, ensure, Context as _, Result};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use terraphim_automata::autocomplete::{build_autocomplete_index, AutocompleteIndex};
use terraphim_lsp_core::KgEngine;
use terraphim_types::{NormalizedTermValue, Thesaurus};

mod markdown;
mod request;
use markdown::MarkdownSource;
pub use request::markdown_kg_request;

/// The local files and loaded snapshot behind an editable role.
#[derive(Clone, Debug)]
pub struct RoleSource(Source);

#[derive(Clone, Debug)]
enum Source {
    Json(JsonSource),
    Markdown(MarkdownSource),
}

#[derive(Clone, Debug)]
struct JsonSource {
    path: PathBuf,
    bytes: Vec<u8>,
    json: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KgRecord {
    pub(crate) id: u64,
    pub(crate) canonical: String,
    pub(crate) url: String,
    pub(crate) synonyms: Vec<String>,
}

impl RoleSource {
    /// Load a standalone thesaurus JSON file or a directory of Markdown KG records.
    ///
    /// # Errors
    /// Returns an error if the source cannot be read or compiled.
    pub fn load(path: &Path) -> Result<Self> {
        if path.is_dir() {
            let canonical = path.canonicalize()?;
            let name = canonical
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Knowledge Graph");
            Ok(Self(Source::Markdown(MarkdownSource::load(
                &canonical, name,
            )?)))
        } else {
            Ok(Self(Source::Json(JsonSource::load(path)?)))
        }
    }

    /// Return the compiled thesaurus for the currently loaded source snapshot.
    ///
    /// # Errors
    /// Returns an error if the JSON source cannot be decoded.
    pub fn thesaurus(&self) -> Result<Thesaurus> {
        match &self.0 {
            Source::Json(source) => source.thesaurus(),
            Source::Markdown(source) => Ok(source.thesaurus()),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        match &self.0 {
            Source::Json(source) => source.path(),
            Source::Markdown(source) => source.path(),
        }
    }

    pub(crate) fn record_path(&self, id: u64) -> Result<&Path> {
        match &self.0 {
            Source::Json(source) => Ok(source.path()),
            Source::Markdown(source) => source.record_path(id),
        }
    }

    pub(crate) fn record(&self, concept_id: u64) -> Result<KgRecord> {
        match &self.0 {
            Source::Json(source) => {
                ensure!(source.is_editable(), "Load the role's source Markdown directory with --kg-dir or --role NAME=DIR to edit this generated role");
                source.record(concept_id)
            }
            Source::Markdown(source) => source.record(concept_id),
        }
    }

    pub(crate) fn save(&mut self, record: &KgRecord) -> Result<(KgEngine, Arc<AutocompleteIndex>)> {
        match &mut self.0 {
            Source::Json(source) => source.save(record),
            Source::Markdown(source) => source.save(record),
        }
    }
}

impl JsonSource {
    /// Read and validate the local thesaurus.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is inaccessible or the file is not a
    /// Terraphim thesaurus.
    pub fn load(path: &Path) -> Result<Self> {
        let path = path
            .canonicalize()
            .with_context(|| format!("Could not locate {}", path.display()))?;
        let bytes =
            fs::read(&path).with_context(|| format!("Could not read {}", path.display()))?;
        let json = serde_json::from_slice(&bytes)
            .with_context(|| format!("Could not parse {}", path.display()))?;
        let source = Self { path, bytes, json };
        source.thesaurus()?;
        Ok(source)
    }

    /// Read the typed thesaurus from the loaded version.
    ///
    /// # Errors
    ///
    /// Returns an error if the file does not contain a valid thesaurus.
    pub fn thesaurus(&self) -> Result<Thesaurus> {
        serde_json::from_slice(&self.bytes)
            .with_context(|| format!("Could not parse {}", self.path.display()))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn is_editable(&self) -> bool {
        self.json.get("source_hash").is_none_or(Value::is_null)
    }

    pub(crate) fn record(&self, concept_id: u64) -> Result<KgRecord> {
        let concept = self.concept(concept_id)?;
        let canonical = concept
            .template
            .get("display_value")
            .and_then(Value::as_str)
            .filter(|display| !display.trim().is_empty())
            .unwrap_or(concept.name);
        let url = concept
            .template
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let synonyms: Vec<_> = concept
            .entries
            .keys()
            .filter(|key| is_synonym(key, concept.name))
            .map(|key| (*key).to_owned())
            .collect();
        ensure!(
            synonyms
                .iter()
                .all(|synonym| synonym.chars().all(|character| !character.is_control())),
            "This knowledge graph record contains a synonym that is not printable on a single line"
        );
        Ok(KgRecord {
            id: concept_id,
            canonical: canonical.to_owned(),
            url: url.to_owned(),
            synonyms,
        })
    }

    pub(crate) fn save(&mut self, record: &KgRecord) -> Result<(KgEngine, Arc<AutocompleteIndex>)> {
        ensure!(
            self.is_editable(),
            "This role is generated from Markdown. Edit the source records and rebuild the thesaurus."
        );
        let original = self.record(record.id)?;
        ensure!(
            record.canonical == original.canonical && record.url == original.url,
            "Only the synonyms of a knowledge graph record can be changed here"
        );

        let candidate = self.apply_synonyms(record)?;
        let thesaurus: Thesaurus = serde_json::from_value(candidate.clone())?;
        let graph = KgEngine::new(&thesaurus)?;
        let index = Arc::new(build_autocomplete_index(thesaurus, None)?);
        if candidate == self.json {
            self.check_unchanged()?;
        } else {
            let mut bytes = serde_json::to_vec_pretty(&candidate)?;
            bytes.push(b'\n');
            self.write(&bytes)?;
            self.bytes = bytes;
            self.json = candidate;
        }
        Ok((graph, index))
    }

    fn apply_synonyms(&self, record: &KgRecord) -> Result<Value> {
        let concept = self.concept(record.id)?;
        let data = role_data(&self.json)?;
        let requested = concept.requested_synonyms(&record.synonyms)?;
        let mut candidate = self.json.clone();
        let updated = candidate
            .get_mut("data")
            .and_then(Value::as_object_mut)
            .context("Role file does not contain a term map")?;

        for (key, _) in concept
            .entries
            .iter()
            .filter(|(key, _)| is_synonym(key, concept.name))
        {
            if !requested.contains_key(normalize(key).as_str()) {
                updated.remove(*key);
            }
        }

        for (normalized, existing) in requested {
            if existing {
                continue;
            }
            ensure_available(data, &normalized, record.id)?;
            ensure!(
                normalized.len() >= terraphim_automata::compiled::DEFAULT_MIN_PATTERN_LENGTH,
                "Synonym {normalized:?} is too short for knowledge graph matching"
            );
            updated.insert(normalized, concept.template.clone());
        }

        if !updated
            .values()
            .any(|entry| concept_id(entry) == Some(record.id))
        {
            // Some older roles only store aliases. Retain their concept
            // when the user removes its final alias.
            let name = normalize(concept.name);
            ensure_available(data, &name, record.id)?;
            updated.insert(name, concept.template.clone());
        }
        Ok(candidate)
    }

    fn concept(&self, id: u64) -> Result<Concept<'_>> {
        let data = role_data(&self.json)?;
        let entries: BTreeMap<_, _> = data
            .iter()
            .filter(|(_, entry)| concept_id(entry) == Some(id))
            .map(|(key, entry)| (key.as_str(), entry))
            .collect();
        let first = entries
            .values()
            .next()
            .context("Knowledge graph record no longer exists")?;
        let name = canonical_name(first).context("Knowledge graph record has no canonical term")?;
        let normalized_name = normalize(name);
        ensure!(
            !normalized_name.is_empty()
                && entries.values().all(|entry| {
                    canonical_name(entry).is_some_and(|value| normalize(value) == normalized_name)
                }),
            "Knowledge graph record has inconsistent canonical terms"
        );
        let template = entries
            .get(normalized_name.as_str())
            .copied()
            .or_else(|| {
                entries
                    .iter()
                    .find_map(|(key, value)| (normalize(key) == normalized_name).then_some(*value))
            })
            .unwrap_or(first);
        let name =
            canonical_name(template).context("Knowledge graph record has no canonical term")?;
        Ok(Concept {
            entries,
            name,
            template,
        })
    }

    fn check_unchanged(&self) -> Result<()> {
        let current = fs::read(&self.path)
            .with_context(|| format!("Could not read {}", self.path.display()))?;
        ensure!(
            current == self.bytes,
            "The role file changed on disk. Reload the editor before editing its synonyms."
        );
        Ok(())
    }

    fn write(&self, bytes: &[u8]) -> Result<()> {
        let source = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .with_context(|| format!("Could not open {} for writing", self.path.display()))?;
        source
            .try_lock()
            .context("Another editor is writing this knowledge graph")?;
        let directory = self
            .path
            .parent()
            .context("Role file has no parent directory")?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary
            .as_file()
            .set_permissions(source.metadata()?.permissions())?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        self.check_unchanged()?;
        temporary
            .persist(&self.path)
            .with_context(|| format!("Could not save {}", self.path.display()))?;
        Ok(())
    }
}

struct Concept<'a> {
    entries: BTreeMap<&'a str, &'a Value>,
    name: &'a str,
    template: &'a Value,
}

impl Concept<'_> {
    fn requested_synonyms(&self, synonyms: &[String]) -> Result<BTreeMap<String, bool>> {
        let mut requested = BTreeMap::new();
        for synonym in synonyms {
            let normalized = normalize(synonym);
            if normalized.is_empty() {
                continue;
            }
            ensure!(
                synonym
                    .trim()
                    .chars()
                    .all(|character| !character.is_control()),
                "Synonyms must be printable text on a single line"
            );
            ensure!(
                is_synonym(&normalized, self.name),
                "{synonym:?} is the canonical term"
            );
            let existing = self.entries.keys().any(|key| normalize(key) == normalized);
            ensure!(
                requested.insert(normalized, existing).is_none(),
                "Synonym {synonym:?} is listed more than once"
            );
        }
        Ok(requested)
    }
}

fn role_data(json: &Value) -> Result<&Map<String, Value>> {
    json.get("data")
        .and_then(Value::as_object)
        .context("Role file does not contain a term map")
}

fn concept_id(entry: &Value) -> Option<u64> {
    entry.get("id")?.as_u64()
}

fn canonical_name(entry: &Value) -> Option<&str> {
    entry.get("nterm")?.as_str()
}

fn normalize(text: &str) -> String {
    NormalizedTermValue::from(text).to_string()
}

fn is_synonym(key: &str, canonical: &str) -> bool {
    normalize(key) != normalize(canonical)
}

fn ensure_available(data: &Map<String, Value>, synonym: &str, owner: u64) -> Result<()> {
    for (key, term) in data {
        let other = canonical_name(term).unwrap_or(key);
        if concept_id(term) != Some(owner)
            && (normalize(key) == synonym || normalize(other) == synonym)
        {
            bail!("Synonym {synonym:?} already belongs to {other:?}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use terraphim_automata::autocomplete::autocomplete_search;

    fn fixture() -> Value {
        json!({
            "name": "Tools",
            "other_metadata": {"scope": "private"},
            "data": {
                "eraser": {"id": 7, "nterm": "eraser", "display_value": "Eraser", "url": "file:///kg/eraser.md", "pinned": true},
                "rubber": {"id": 7, "nterm": "eraser", "trigger": "rub", "unknown_metadata": [1, 2]},
                "scrubber": {"id": 7, "nterm": "eraser"},
                "paperclip": {"id": 8, "nterm": "paperclip", "action": "preserve me", "extra": 42},
            },
        })
    }

    fn source(directory: &Path, json: &Value) -> JsonSource {
        let path = directory.join("role.json");
        fs::write(&path, serde_json::to_vec(json).unwrap()).unwrap();
        JsonSource::load(&path).unwrap()
    }

    #[test]
    fn saves_synonyms_and_rebuilds_the_shared_graph_and_autocomplete() {
        let directory = tempfile::tempdir().unwrap();
        let original = fixture();
        let mut role = source(directory.path(), &original);
        let mut record = role.record(7).unwrap();
        assert_eq!(record.canonical, "Eraser");
        assert_eq!(record.synonyms, ["rubber", "scrubber"]);
        record.synonyms = vec!["rubber".into(), "  Wiper ".into()];
        let (graph, autocomplete) = role.save(&record).unwrap();
        assert_eq!(graph.analyse("Use a wiper.").matches[0].concept_id, 7);
        assert_eq!(graph.analyse("Use a scrubber.").matches, []);
        assert_eq!(
            autocomplete_search(&autocomplete, "wi", Some(5))
                .unwrap()
                .len(),
            1
        );
        let persisted: Value = serde_json::from_slice(&fs::read(role.path()).unwrap()).unwrap();
        for key in ["eraser", "rubber", "paperclip"] {
            assert_eq!(persisted["data"][key], original["data"][key]);
        }
        assert_eq!(persisted["other_metadata"], original["other_metadata"]);
        assert_eq!(persisted["data"]["wiper"]["id"], 7);
        assert_eq!(
            JsonSource::load(role.path())
                .unwrap()
                .record(7)
                .unwrap()
                .synonyms,
            ["rubber", "wiper"]
        );
    }

    #[test]
    fn rejects_collisions_and_never_overwrites_a_concurrent_save() {
        let directory = tempfile::tempdir().unwrap();
        let mut role = source(directory.path(), &fixture());
        let mut other = JsonSource::load(role.path()).unwrap();
        let mut record = role.record(7).unwrap();
        record.synonyms.push(" PAPeRCLIP ".into());
        let error = role.save(&record).err().unwrap().to_string();
        assert!(error.contains("already belongs to"));
        record.synonyms.pop();
        record.synonyms.push("wiper".into());
        other.save(&record).unwrap();
        record.synonyms.push("rub-off".into());
        let error = role.save(&record).err().unwrap().to_string();
        assert!(error.contains("changed on disk"));
        assert_eq!(
            JsonSource::load(role.path())
                .unwrap()
                .record(7)
                .unwrap()
                .synonyms,
            ["rubber", "scrubber", "wiper"]
        );
    }

    #[test]
    fn generated_markdown_roles_are_read_only() {
        let directory = tempfile::tempdir().unwrap();
        let mut json = fixture();
        json["source_hash"] = json!("abc");
        let mut role = source(directory.path(), &json);
        let record = role.record(7).unwrap();
        assert!(!role.is_editable());
        let error = role.save(&record).err().unwrap().to_string();
        assert!(error.contains("generated from Markdown"));
    }

    #[test]
    fn removing_last_alias_keeps_a_legacy_concepts_identity() {
        let directory = tempfile::tempdir().unwrap();
        let mut json = fixture();
        json["data"].as_object_mut().unwrap().remove("eraser");
        let mut role = source(directory.path(), &json);
        let mut record = role.record(7).unwrap();
        record.synonyms.clear();
        let (graph, _) = role.save(&record).unwrap();
        assert_eq!(graph.analyse("An eraser.").matches[0].concept_id, 7);
        assert_eq!(role.record(7).unwrap().synonyms, [] as [String; 0]);
    }

    #[test]
    fn preserves_legacy_case_and_refuses_ambiguous_concepts() {
        let directory = tempfile::tempdir().unwrap();
        let mut json = fixture();
        json["data"]["rubber"]["nterm"] = json!("Eraser");
        let role = source(directory.path(), &json);
        assert_eq!(role.record(7).unwrap().canonical, "Eraser");
        assert_eq!(role.record(7).unwrap().synonyms, ["rubber", "scrubber"]);
        json["data"]["rubber"]["nterm"] = json!("wiper");
        let role = source(directory.path(), &json);
        let error = role.record(7).unwrap_err().to_string();
        assert!(error.contains("inconsistent canonical terms"));
    }
}
