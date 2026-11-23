use std::ops::Range;
use terraphim_alternatives::{
    byte_to_utf16, parse, utf16_to_byte, BlockError, Document, Source, SpanKind,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum OpenError {
    #[error(transparent)]
    Block(#[from] BlockError),
    #[error("Some annotations could not be located in the Markdown body")]
    Unresolved,
}

pub(crate) fn open(markdown: &str) -> Result<Document, OpenError> {
    let mut document = parse(markdown)?;
    if !document.annotations.spans.is_empty() || !document.annotations.ghosts.is_empty() {
        let report = document.reanchor();
        if !report.unresolved.is_empty() || !report.unresolved_ghosts.is_empty() {
            return Err(OpenError::Unresolved);
        }
    }
    Ok(document)
}

// GPUI reports the final buffer. Compare on character boundaries, then
// submit the changed range in the shared model's UTF-16 coordinates.
pub(crate) fn sync(document: &mut Document, body: String) -> anyhow::Result<()> {
    if document.body == body {
        return Ok(());
    }
    if document.annotations.is_empty() {
        document.body = body;
        return Ok(());
    }

    replace_region(document, 0..document.body.len(), &body)
}

/// Forms a minimal edit inside the selected byte range.
pub(crate) fn replace_region(
    document: &mut Document,
    range: Range<usize>,
    body: &str,
) -> anyhow::Result<()> {
    let original = document
        .body
        .get(range.clone())
        .ok_or_else(|| anyhow::anyhow!("The block changed"))?;
    if original == body {
        return Ok(());
    }
    let prefix = original
        .chars()
        .zip(body.chars())
        .take_while(|(old, new)| old == new)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let suffix = original[prefix..]
        .chars()
        .rev()
        .zip(body[prefix..].chars().rev())
        .take_while(|(old, new)| old == new)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let start = byte_to_utf16(&document.body, range.start + prefix).expect("character boundary");
    let end = byte_to_utf16(&document.body, range.end - suffix).expect("character boundary");
    apply_preserving_spans(document, start, end, &body[prefix..body.len() - suffix])
}

pub(crate) fn replace_command(
    document: &mut Document,
    command: &str,
    replacement: &str,
) -> anyhow::Result<()> {
    let mut start = 0;
    for line in document.body.split_inclusive('\n') {
        let end = start + terraphim_alternatives::utf16_len(line);
        if line.trim() == command {
            let (start, end) = if replacement.is_empty() {
                (start, end)
            } else {
                let command_start = start
                    + terraphim_alternatives::utf16_len(
                        &line[..line.len() - line.trim_start().len()],
                    );
                (
                    command_start,
                    command_start + terraphim_alternatives::utf16_len(command),
                )
            };
            apply_preserving_spans(document, start, end, replacement)?;
            break;
        }
        start = end;
    }
    Ok(())
}

fn apply_preserving_spans(
    document: &mut Document,
    start: usize,
    end: usize,
    replacement: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !document
            .annotations
            .spans
            .iter()
            .any(|span| { span.anchor.start < end && start < span.anchor.end }),
        "This edit would remove saved alternatives. Undo the edit or use the Annotations menu."
    );
    document.apply_edit(start, end, replacement)?;
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum Operation {
    AddAlternative,
    ClearAlternatives,
    Cycle(isize),
    Ghost,
    Revive,
    Stash,
    Pull,
}

impl Operation {
    pub(crate) fn apply(
        self,
        document: &mut Document,
        selection: Range<usize>,
        alternative: Option<String>,
    ) -> anyhow::Result<()> {
        let Range { start, end } = selection;
        let span_id = document
            .annotations
            .spans
            .iter()
            .find(|span| {
                span.anchor.start <= start && start < span.anchor.end && end <= span.anchor.end
            })
            .map(|span| span.id.clone());

        match self {
            Self::AddAlternative => {
                let text = alternative.filter(|text| !text.is_empty()).ok_or_else(|| {
                    anyhow::anyhow!("Copy the replacement text before adding an alternative")
                })?;
                let id = if let Some(id) = span_id {
                    id
                } else {
                    let selected = selected_text(document, start, end)?;
                    let kind = match terraphim_alternatives::text::infer_kind(selected) {
                        terraphim_alternatives::text::TextUnit::Word => SpanKind::Word,
                        terraphim_alternatives::text::TextUnit::Sentence => SpanKind::Sentence,
                        terraphim_alternatives::text::TextUnit::Paragraph => SpanKind::Paragraph,
                    };
                    document.add_span(kind, start, end)?
                };
                let index = document.add_alternative(&id, text, Source::Human, None)?;
                document.set_active(&id, index)?;
            }
            Self::Cycle(step) => {
                let id = span_id.ok_or_else(|| {
                    anyhow::anyhow!("Place the cursor inside text with alternatives")
                })?;
                document.cycle_active(&id, step)?;
            }
            Self::ClearAlternatives => {
                let id = span_id.ok_or_else(|| {
                    anyhow::anyhow!("Place the cursor inside text with alternatives")
                })?;
                document.clear_alternatives(&id)?;
            }
            Self::Ghost => {
                document.ghost(start, end)?;
            }
            Self::Revive => {
                let range = if selection.is_empty() {
                    let ghost = document
                        .ghost_at(start)
                        .ok_or_else(|| anyhow::anyhow!("Place the cursor inside ghosted text"))?;
                    ghost.anchor.start..ghost.anchor.end
                } else {
                    selection
                };
                document.revive(range.start, range.end)?;
            }
            Self::Stash => {
                let text = selected_text(document, start, end)?.to_owned();
                anyhow::ensure!(
                    !document
                        .annotations
                        .ghosts
                        .iter()
                        .any(|ghost| { ghost.anchor.start < end && start < ghost.anchor.end }),
                    "Revive ghosted text before stashing it"
                );
                apply_preserving_spans(document, start, end, "")?;
                if !document.annotations.overflow.is_empty() {
                    document.annotations.overflow.push_str("\n\n");
                }
                document.annotations.overflow.push_str(&text);
            }
            Self::Pull => {
                anyhow::ensure!(
                    !document.annotations.overflow.is_empty(),
                    "Overflow is empty"
                );
                let text = document.annotations.overflow.clone();
                apply_preserving_spans(document, start, start, &text)?;
                document.annotations.overflow.clear();
            }
        }
        Ok(())
    }
}

fn selected_text(document: &Document, start: usize, end: usize) -> anyhow::Result<&str> {
    let text = utf16_to_byte(&document.body, start)
        .zip(utf16_to_byte(&document.body, end))
        .and_then(|(start, end)| document.body.get(start..end))
        .filter(|text| !text.is_empty());
    text.ok_or_else(|| anyhow::anyhow!("Select text first"))
}

/// A snapshot includes the body and all annotation layers so metadata-only
/// operations and text substitutions share one undo/redo sequence.
#[derive(Default)]
pub(crate) struct History {
    undo: std::collections::VecDeque<Document>,
    redo: Vec<Document>,
}

impl History {
    pub(crate) fn record(&mut self, before: Document, after: &Document) {
        const LIMIT: usize = 64;
        if &before != after {
            if self.undo.len() == LIMIT {
                self.undo.pop_front();
            }
            self.undo.push_back(before);
            self.redo.clear();
            self.trim();
        }
    }

    pub(crate) fn travel(&mut self, document: &mut Document, redo: bool) -> bool {
        if redo {
            let Some(next) = self.redo.pop() else {
                return false;
            };
            self.undo.push_back(std::mem::replace(document, next));
        } else {
            let Some(previous) = self.undo.pop_back() else {
                return false;
            };
            self.redo.push(std::mem::replace(document, previous));
        }
        self.trim();
        true
    }

    fn trim(&mut self) {
        const BYTE_LIMIT: usize = 32 * 1024 * 1024;
        let mut bytes = self
            .undo
            .iter()
            .chain(&self.redo)
            .map(snapshot_bytes)
            .sum::<usize>();
        while bytes > BYTE_LIMIT {
            let Some(oldest) = self
                .undo
                .pop_front()
                .or_else(|| (!self.redo.is_empty()).then(|| self.redo.remove(0)))
            else {
                break;
            };
            bytes -= snapshot_bytes(&oldest);
        }
    }
}

fn snapshot_bytes(document: &Document) -> usize {
    let annotations = &document.annotations;
    let anchor_bytes = |anchor: &terraphim_alternatives::Anchor| {
        anchor.text.len()
            + anchor.before.as_ref().map_or(0, String::len)
            + anchor.after.as_ref().map_or(0, String::len)
    };
    document.body.len()
        + annotations.overflow.len()
        + annotations
            .spans
            .iter()
            .map(|span| {
                std::mem::size_of_val(span)
                    + span.id.len()
                    + anchor_bytes(&span.anchor)
                    + span
                        .alts
                        .iter()
                        .map(|alternative| {
                            std::mem::size_of_val(alternative)
                                + alternative.text.len()
                                + alternative.model.as_ref().map_or(0, String::len)
                        })
                        .sum::<usize>()
            })
            .sum::<usize>()
        + annotations
            .ghosts
            .iter()
            .map(|ghost| {
                std::mem::size_of_val(ghost) + ghost.id.len() + anchor_bytes(&ghost.anchor)
            })
            .sum::<usize>()
}

pub(crate) fn span_at<'a>(
    document: &'a Document,
    selection: &Range<usize>,
) -> Option<&'a terraphim_alternatives::Span> {
    document.annotations.spans.iter().find(|span| {
        span.anchor.start <= selection.start
            && selection.start < span.anchor.end
            && selection.end <= span.anchor.end
    })
}

pub(crate) fn cycle_kg(
    document: &mut Document,
    engine: &terraphim_lsp_core::KgEngine,
    offset: usize,
    forward: bool,
) -> anyhow::Result<usize> {
    let set = engine
        .alternatives_at_utf16(&document.body, offset)
        .filter(|set| !set.replacements.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Place the cursor inside a knowledge-graph synonym"))?;
    let terms = engine.concept_index().synonyms_of(set.term.concept_id);
    let current = terms
        .iter()
        .position(|term| term.as_str() == set.term.term)
        .ok_or_else(|| anyhow::anyhow!("The current term is absent from its synonym group"))?;
    let next = if forward {
        (current + 1) % terms.len()
    } else {
        (current + terms.len() - 1) % terms.len()
    };
    let replacement = &set.replacements[if next < current { next } else { next - 1 }];
    let mut caret = set.term.range.start.utf16;
    for edit in &replacement.edits {
        if edit.range.end.utf16 <= set.term.range.start.utf16 {
            caret = caret - (edit.range.end.utf16 - edit.range.start.utf16)
                + terraphim_alternatives::utf16_len(&edit.new_text);
        }
    }
    let mut candidate = document.clone();
    for edit in replacement.edits.iter().rev() {
        apply_preserving_spans(
            &mut candidate,
            edit.range.start.utf16,
            edit.range.end.utf16,
            &edit.new_text,
        )?;
    }
    *document = candidate;
    Ok(caret)
}

#[cfg(test)]
mod tests {
    use super::{cycle_kg, open, replace_command, sync, History, Operation};
    use terraphim_alternatives::{write, Document};

    #[test]
    fn annotated_markdown_round_trips_after_unicode_edit() {
        let mut document = Document::new("🦀 Notes. Pass me a paperclip.");
        Operation::AddAlternative
            .apply(&mut document, 20..29, Some("eraser".into()))
            .unwrap();
        assert_eq!(document.body, "🦀 Notes. Pass me an eraser.");
        Operation::Ghost.apply(&mut document, 3..9, None).unwrap();
        let source = write(&document);
        let mut reopened = open(&source).unwrap();
        assert_eq!(reopened, document);

        let body = format!("é {}", reopened.body);
        sync(&mut reopened, body).unwrap();
        assert_eq!(reopened.annotations.spans[0].anchor.text, "eraser");
        assert_eq!(reopened.annotations.ghosts[0].anchor.start, 5);
        assert_eq!(reopened.export(), "é 🦀 Pass me an eraser.");
        assert_eq!(open(&write(&reopened)).unwrap(), reopened);
    }

    #[test]
    fn cycles_and_revives_existing_annotations() {
        let mut document = Document::new("a clip");
        Operation::AddAlternative
            .apply(&mut document, 2..6, Some("eraser".into()))
            .unwrap();
        Operation::Ghost.apply(&mut document, 0..9, None).unwrap();
        Operation::Cycle(-1)
            .apply(&mut document, 4..4, None)
            .unwrap();
        assert_eq!(document.body, "a clip");
        assert_eq!(document.export(), "");
        Operation::Revive.apply(&mut document, 3..3, None).unwrap();
        assert_eq!(document.export(), "a clip");
    }

    #[test]
    fn stashes_and_restores_selected_text_without_exporting_it() {
        let mut document = Document::new("Keep 🦀.");
        Operation::Stash.apply(&mut document, 4..7, None).unwrap();
        assert_eq!(document.body, "Keep.");
        let mut document = open(&write(&document)).unwrap();
        assert_eq!(document.annotations.overflow, " 🦀");
        assert_eq!(document.export(), "Keep.");
        Operation::Pull.apply(&mut document, 4..4, None).unwrap();
        assert_eq!(document.body, "Keep 🦀.");
        assert!(document.annotations.is_empty());
    }

    #[test]
    fn malformed_annotations_are_rejected() {
        assert!(open("Text\n\n```terraphim-alternatives\ninvalid JSON\n```").is_err());
    }

    #[test]
    fn save_removes_only_the_command_line_and_keeps_later_annotations() {
        let mut document = Document::new("A /save example\n/save\nnotes");
        Operation::Ghost.apply(&mut document, 22..27, None).unwrap();
        replace_command(&mut document, "/save", "").unwrap();
        assert_eq!(document.body, "A /save example\nnotes");
        assert_eq!(document.annotations.ghosts[0].anchor.start, 16);
        assert_eq!(open(&write(&document)).unwrap(), document);
    }

    #[test]
    fn refuses_edits_and_stashes_that_would_discard_alternatives() {
        let mut document = Document::new("a clip");
        Operation::AddAlternative
            .apply(&mut document, 2..6, Some("eraser".into()))
            .unwrap();
        let before = document.clone();
        assert!(sync(&mut document, "an erase".into()).is_err());
        assert!(Operation::Stash.apply(&mut document, 3..9, None).is_err());
        assert_eq!(document, before);
    }

    #[test]
    fn history_replays_metadata_only_actions_and_discards_redo_on_edit() {
        let mut doc = Document::new("a clip");
        let mut history = History::default();
        let before = doc.clone();
        Operation::Ghost.apply(&mut doc, 0..6, None).unwrap();
        history.record(before, &doc);
        assert!(history.travel(&mut doc, false));
        assert_eq!(doc.export(), "a clip");
        assert!(history.travel(&mut doc, true));
        assert_eq!(doc.export(), "");
        assert!(history.travel(&mut doc, false));
        let before = doc.clone();
        Operation::Stash.apply(&mut doc, 2..6, None).unwrap();
        history.record(before, &doc);
        assert!(!history.travel(&mut doc, true));
        assert!(history.travel(&mut doc, false));
        assert_eq!(doc.body, "a clip");
        assert_eq!(doc.annotations.overflow, "");
    }

    #[test]
    fn kg_cycle_wraps_with_case_article_and_utf16_caret() {
        let engine = terraphim_lsp_core::KgEngine::from_json(
            r#"{"name":"Tools","data":{
            "eraser":{"id":1,"nterm":"eraser"},"rubber":{"id":1,"nterm":"eraser"}
        }}"#,
        )
        .unwrap();
        let mut doc = Document::new("🐈 An ERASER.");
        let caret = cycle_kg(&mut doc, &engine, 6, true).unwrap();
        assert_eq!(doc.body, "🐈 A RUBBER.");
        assert_eq!(caret, 5);
        assert!(doc.annotations.is_empty());
        let caret = cycle_kg(&mut doc, &engine, caret, true).unwrap();
        assert_eq!(doc.body, "🐈 An ERASER.");
        assert_eq!(caret, 6);
        Operation::AddAlternative
            .apply(&mut doc, 6..12, Some("ERASER".into()))
            .unwrap();
        let before = doc.clone();
        assert!(cycle_kg(&mut doc, &engine, 6, true).is_err());
        assert_eq!(doc, before);
    }
}
