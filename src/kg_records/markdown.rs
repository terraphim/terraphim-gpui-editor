//! Authoritative Markdown role sources.

use super::{normalize, KgRecord};
use anyhow::{ensure, Context as _, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use terraphim_automata::autocomplete::{build_autocomplete_index, AutocompleteIndex};
use terraphim_automata::compiled::DEFAULT_MIN_PATTERN_LENGTH;
use terraphim_automata::parse_markdown_directives_str;
use terraphim_lsp_core::KgEngine;
use terraphim_types::{DocumentType, NormalizedTerm, NormalizedTermValue, Thesaurus};
use url::Url;

const SYNONYMS_PREFIX: &str = "synonyms::";

/// A compiled role and the exact source snapshot used to build it.
#[derive(Clone, Debug)]
pub(super) struct MarkdownSource {
    root: PathBuf,
    documents: BTreeMap<PathBuf, String>,
    thesaurus: Thesaurus,
    concepts: BTreeMap<u64, Concept>,
}

#[derive(Clone, Debug)]
struct Concept {
    path: PathBuf,
    term: NormalizedTerm,
    synonyms: Vec<String>,
}

impl MarkdownSource {
    pub(super) fn load(root: &Path, name: &str) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("Could not locate {}", root.display()))?;
        let documents = read_documents(&root)?;
        let (thesaurus, concepts) = compile(name, &documents)?;
        Ok(Self {
            root,
            documents,
            thesaurus,
            concepts,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.root
    }

    pub(super) fn thesaurus(&self) -> Thesaurus {
        self.thesaurus.clone()
    }

    pub(super) fn revision(&self) -> String {
        let mut digest = Sha256::new();
        for (path, source) in &self.documents {
            let path = path
                .strip_prefix(&self.root)
                .unwrap_or(path)
                .as_os_str()
                .as_encoded_bytes();
            digest.update(path.len().to_le_bytes());
            digest.update(path);
            digest.update(source.len().to_le_bytes());
            digest.update(source.as_bytes());
        }
        format!("{:x}", digest.finalize())
    }

    pub(super) fn record_path(&self, id: u64) -> Result<&Path> {
        Ok(&self.concept(id)?.path)
    }

    pub(super) fn record(&self, id: u64) -> Result<KgRecord> {
        let concept = self.concept(id)?;
        Ok(KgRecord {
            id,
            canonical: concept.term.display().to_owned(),
            url: concept.term.url.clone().unwrap_or_default(),
            synonyms: concept.synonyms.clone(),
        })
    }

    pub(super) fn save(&mut self, record: &KgRecord) -> Result<(KgEngine, Arc<AutocompleteIndex>)> {
        let old = self.record(record.id)?;
        ensure!(
            old.canonical == record.canonical && old.url == record.url,
            "Only the synonyms of a knowledge graph record can be changed here"
        );
        let synonyms = self.validate_synonyms(record)?;
        let path = self.record_path(record.id)?.to_owned();
        let mut documents = self.documents.clone();
        let source = documents
            .get_mut(&path)
            .context("Knowledge graph source no longer exists")?;
        if synonyms != old.synonyms {
            let spellings = synonym_spellings(source);
            let written: Vec<_> = synonyms
                .iter()
                .zip(&record.synonyms)
                .map(|(normalized, raw)| {
                    spellings.get(normalized).map_or(raw.trim(), String::as_str)
                })
                .collect();
            *source = replace_synonyms(source, &written);
        }
        let (thesaurus, concepts) = compile(self.thesaurus.name(), &documents)?;
        ensure!(
            concepts.get(&record.id).is_some_and(|concept| concept.synonyms == synonyms),
            "The edited Markdown does not expose these synonyms as a directive. Close any unfinished code or HTML blocks and try again."
        );
        let graph = KgEngine::new(&thesaurus)?;
        let index = Arc::new(build_autocomplete_index(thesaurus.clone(), None)?);
        if documents == self.documents {
            self.check_unchanged()?;
        } else {
            self.write(&path, &documents[&path])?;
        }

        self.documents = documents;
        self.thesaurus = thesaurus;
        self.concepts = concepts;
        Ok((graph, index))
    }

    fn concept(&self, id: u64) -> Result<&Concept> {
        self.concepts
            .get(&id)
            .context("Knowledge graph record no longer exists")
    }

    fn validate_synonyms(&self, record: &KgRecord) -> Result<Vec<String>> {
        let concept = self.concept(record.id)?;
        let mut seen = BTreeSet::new();
        let mut synonyms = Vec::with_capacity(record.synonyms.len());
        for raw in &record.synonyms {
            ensure!(
                !raw.contains(',') && !raw.chars().any(char::is_control),
                "A Markdown synonym must be a single line without commas"
            );
            let synonym = normalize(raw);
            ensure!(!synonym.is_empty(), "A synonym cannot be empty");
            ensure!(
                synonym != concept.term.value.as_str(),
                "{raw:?} is the canonical term, which is already searchable"
            );
            ensure!(seen.insert(synonym.clone()), "Duplicate synonym {raw:?}");

            if !concept.synonyms.contains(&synonym) {
                ensure!(
                    synonym.len() >= DEFAULT_MIN_PATTERN_LENGTH,
                    "Synonym {raw:?} is too short for knowledge graph matching"
                );
                if let Some(other) = self.concepts.values().find(|other| {
                    other.term.id != record.id
                        && (other.term.value.as_str() == synonym
                            || other.synonyms.contains(&synonym))
                }) {
                    anyhow::bail!(
                        "Synonym {raw:?} belongs to a different knowledge graph record ({})",
                        other.path.display()
                    );
                }
            }
            synonyms.push(synonym);
        }
        Ok(synonyms)
    }

    fn check_unchanged(&self) -> Result<()> {
        ensure!(
            read_documents(&self.root)? == self.documents,
            "The Markdown role changed on disk. Reload the editor before editing its synonyms."
        );
        Ok(())
    }

    fn write(&self, path: &Path, source: &str) -> Result<()> {
        // Serialize concurrent editor saves across all records in this role.
        #[cfg(unix)]
        let role_lock = fs::File::open(&self.root)?;
        #[cfg(not(unix))]
        let role_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(".terraphim-editor-role.lock"))?;
        role_lock
            .try_lock()
            .context("Another editor is writing this knowledge graph")?;
        let current = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| format!("Could not open {} for writing", path.display()))?;
        current
            .try_lock()
            .context("Another editor is writing this knowledge graph")?;
        let directory = path
            .parent()
            .context("Markdown record has no parent directory")?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary
            .as_file()
            .set_permissions(current.metadata()?.permissions())?;
        temporary.write_all(source.as_bytes())?;
        temporary.as_file().sync_all()?;
        self.check_unchanged()?;
        temporary
            .persist(path)
            .with_context(|| format!("Could not save {}", path.display()))?;
        Ok(())
    }
}

fn read_documents(root: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let mut directories = vec![root.to_owned()];
    let mut documents = BTreeMap::new();
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("Could not read Markdown role {}", directory.display()))?
        {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            {
                let content = fs::read_to_string(&path).with_context(|| {
                    format!("Could not read Markdown record {}", path.display())
                })?;
                documents.insert(path, content);
            }
        }
    }
    Ok(documents)
}

fn compile(
    name: &str,
    documents: &BTreeMap<PathBuf, String>,
) -> Result<(Thesaurus, BTreeMap<u64, Concept>)> {
    let mut concepts = BTreeMap::new();
    for (path, source) in documents {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .with_context(|| format!("Invalid Markdown record filename {}", path.display()))?;
        let mut parsed = parse_markdown_directives_str(stem, &directives_source(source));
        let directives = parsed
            .directives
            .remove(stem)
            .context("Missing Markdown directives")?;
        if directives.doc_type == DocumentType::Document {
            continue;
        }
        let value = NormalizedTermValue::from(stem);
        ensure!(
            !value.as_str().is_empty(),
            "A Markdown record has no canonical term"
        );
        let mut term = NormalizedTerm::with_stable_id(value)
            .with_display_value(directives.heading.unwrap_or_else(|| stem.to_owned()))
            .with_pinned(directives.pinned);
        term.url = Url::from_file_path(path).ok().map(Into::into);
        term.priority = directives.priority;
        term.trigger = directives.trigger;
        term.action = directives.route.and_then(|route| route.action);
        let mut seen = BTreeSet::new();
        let synonyms = directives
            .synonyms
            .iter()
            .map(|synonym| normalize(synonym))
            .filter(|synonym| synonym != term.value.as_str() && seen.insert(synonym.clone()))
            .collect();
        let id = term.id;
        let concept = Concept {
            path: path.clone(),
            term,
            synonyms,
        };
        ensure!(
            !concepts.contains_key(&id),
            "Multiple Markdown records represent {:?}, including {}",
            stem,
            path.display()
        );
        concepts.insert(id, concept);
    }

    let mut thesaurus = Thesaurus::new(name.to_owned());
    for concept in concepts.values() {
        thesaurus.insert(concept.term.value.clone(), concept.term.clone());
    }
    // Keep existing overlapping aliases readable, letting the canonical record
    // take precedence and resolving other legacy aliases by stable concept ID.
    for concept in concepts.values() {
        for synonym in &concept.synonyms {
            let key = NormalizedTermValue::from(synonym.as_str());
            if thesaurus.get(&key).is_none() {
                thesaurus.insert(key, concept.term.clone());
            }
        }
    }
    Ok((thesaurus, concepts))
}

fn protected_ranges(source: &str) -> Vec<Range<usize>> {
    use markdown::mdast::Node;

    fn collect(node: &Node, ranges: &mut Vec<Range<usize>>) {
        if matches!(node, Node::Code(_) | Node::Html(_)) {
            if let Some(position) = node.position() {
                ranges.push(position.start.offset..position.end.offset);
            }
        } else if let Some(children) = node.children() {
            for child in children {
                collect(child, ranges);
            }
        }
    }

    let mut ranges = Vec::new();
    if let Ok(document) = markdown::to_mdast(source, &markdown::ParseOptions::gfm()) {
        collect(&document, &mut ranges);
    }
    ranges
}

fn lines(source: &str) -> impl Iterator<Item = (&str, bool)> {
    let protected = protected_ranges(source);
    let mut offset = 0;
    source.split_inclusive('\n').map(move |line| {
        let start = offset + line.len() - line.trim_start().len();
        let inside = protected.iter().any(|range| range.contains(&start));
        offset += line.len();
        (line, inside)
    })
}

fn directives_source(source: &str) -> String {
    let mut filtered = String::with_capacity(source.len());
    for (line, protected) in lines(source) {
        if protected {
            if line.ends_with('\n') {
                filtered.push('\n');
            }
        } else {
            filtered.push_str(line);
        }
    }
    filtered
}

fn synonym_prefix(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    trimmed
        .get(..SYNONYMS_PREFIX.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(SYNONYMS_PREFIX))?;
    line.get(..line.len() - trimmed.len() + SYNONYMS_PREFIX.len())
}

fn synonym_spellings(source: &str) -> BTreeMap<String, String> {
    let mut spellings = BTreeMap::new();
    for (line, protected) in lines(source) {
        if !protected {
            if let Some(prefix) = synonym_prefix(line) {
                for raw in line[prefix.len()..].split(',') {
                    let raw = raw.trim();
                    spellings
                        .entry(normalize(raw))
                        .or_insert_with(|| raw.to_owned());
                }
            }
        }
    }
    spellings
}

fn replace_synonyms(source: &str, synonyms: &[&str]) -> String {
    let mut result = String::with_capacity(source.len());
    let mut replaced = false;
    let joined = synonyms.join(", ");
    for (line, protected) in lines(source) {
        let Some(prefix) = synonym_prefix(line).filter(|_| !protected) else {
            result.push_str(line);
            continue;
        };
        if !replaced && !synonyms.is_empty() {
            // Preserve indentation, directive spelling, and line ending.
            result.push_str(prefix);
            result.push(' ');
            result.push_str(&joined);
            if line.ends_with("\r\n") {
                result.push_str("\r\n");
            } else if line.ends_with('\n') {
                result.push('\n');
            }
        }
        replaced = true;
    }
    if !replaced && !synonyms.is_empty() {
        let ending = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        if !source.is_empty() && !source.ends_with('\n') {
            result.push_str(ending);
        }
        result.push_str(SYNONYMS_PREFIX);
        result.push(' ');
        result.push_str(&joined);
        result.push_str(ending);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn concept_id(role: &MarkdownSource, name: &str) -> u64 {
        role.thesaurus.get(&name.into()).unwrap().id
    }

    #[test]
    fn edits_exact_source_rebuilds_role_and_preserves_other_markdown() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("eraser.md");
        let original = "# Eraser\r\n\r\nSynonyms:: rubber\r\npriority:: 20\r\n\r\nBody\r\n";
        fs::write(&source, original).unwrap();
        let other = folder.path().join("paperclip.md");
        fs::write(&other, "# Paperclip\nsynonyms:: clip\n").unwrap();

        let mut role = MarkdownSource::load(folder.path(), "Tools").unwrap();
        let id = concept_id(&role, "eraser");
        assert_eq!(
            role.record_path(id).unwrap(),
            source.canonicalize().unwrap()
        );
        let mut record = role.record(id).unwrap();
        assert_eq!(record.synonyms, ["rubber"]);
        record.synonyms = vec!["wiper".into()];
        let (graph, index) = role.save(&record).unwrap();
        assert_eq!(graph.analyse("Pass me a wiper.").matches[0].concept_id, id);
        assert_eq!(graph.analyse("Pass me a rubber.").matches, []);
        assert_eq!(
            terraphim_automata::autocomplete_search(&index, "wi", Some(3))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            original.replace("rubber", "wiper")
        );
        assert_eq!(
            fs::read_to_string(&other).unwrap(),
            "# Paperclip\nsynonyms:: clip\n"
        );
        let reloaded = MarkdownSource::load(folder.path(), "Tools").unwrap();
        assert_eq!(reloaded.record(id).unwrap().synonyms, ["wiper"]);
        assert_eq!(
            reloaded.thesaurus.get(&"wiper".into()).unwrap().priority,
            Some(20)
        );
    }

    #[test]
    fn preserves_source_on_collision_or_external_changes() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("eraser.md");
        let original = "# Eraser\nsynonyms:: rubber\n";
        fs::write(&source, original).unwrap();
        let other = folder.path().join("paperclip.md");
        fs::write(&other, "# Paperclip\nsynonyms:: clip\n").unwrap();
        let mut role = MarkdownSource::load(folder.path(), "Tools").unwrap();
        let mut record = role.record(concept_id(&role, "eraser")).unwrap();
        record.synonyms = vec!["clip".into()];
        assert!(role
            .save(&record)
            .unwrap_err()
            .to_string()
            .contains("belongs"));
        assert_eq!(fs::read_to_string(&source).unwrap(), original);

        fs::write(other, "# Paperclip\nsynonyms:: fastener\n").unwrap();
        record.synonyms = vec!["wiper".into()];
        assert!(role
            .save(&record)
            .unwrap_err()
            .to_string()
            .contains("changed on disk"));
        assert_eq!(fs::read_to_string(source).unwrap(), original);
    }

    #[test]
    fn preserves_code_examples_and_surviving_alias_spelling() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("pointer.md");
        let original =
            "# Pointer\n\nsynonyms:: Arc, stale pointer\n\n```text\nsynonyms:: FakeAlias\n```\n";
        fs::write(&source, original).unwrap();
        let mut role = MarkdownSource::load(folder.path(), "Tools").unwrap();
        let mut record = role.record(concept_id(&role, "pointer")).unwrap();
        assert_eq!(record.synonyms, ["arc", "stale pointer"]);
        record.synonyms = vec!["arc".into(), "New Pointer".into()];
        let (graph, _) = role.save(&record).unwrap();
        assert_eq!(graph.analyse("FakeAlias").matches, []);
        assert_eq!(
            fs::read_to_string(source).unwrap(),
            original.replace("stale pointer", "New Pointer")
        );
    }

    #[test]
    fn refuses_to_append_into_an_unclosed_code_block() {
        let folder = tempfile::tempdir().unwrap();
        let source = folder.path().join("pointer.md");
        let original = "# Pointer\n\n```example\nAn unfinished example.\n";
        fs::write(&source, original).unwrap();
        let mut role = MarkdownSource::load(folder.path(), "Tools").unwrap();
        let mut record = role.record(concept_id(&role, "pointer")).unwrap();
        record.synonyms = vec!["Arc".into()];

        assert!(role
            .save(&record)
            .unwrap_err()
            .to_string()
            .contains("unfinished"));
        assert_eq!(fs::read_to_string(source).unwrap(), original);
    }

    #[test]
    fn consolidates_directives_while_preserving_non_directive_lines() {
        assert_eq!(
            replace_synonyms(
                "# Heading\n synonyms:: alpha\nParagraph\nsynonyms:: beta",
                &["gamma"]
            ),
            "# Heading\n synonyms:: gamma\nParagraph\n"
        );
        assert_eq!(
            replace_synonyms("# Heading", &["word"]),
            "# Heading\nsynonyms:: word\n"
        );
        assert_eq!(
            replace_synonyms("# Heading\nsynonyms:: word\n", &[]),
            "# Heading\n"
        );
    }
}
