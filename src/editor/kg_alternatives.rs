use super::{tree_view, TerraphimEditor};
use crate::completion::TerraphimCompletionProvider;
use crate::dependency_tree::TreeGroup;
use crate::kg_records::{KgRecord, RoleSource};
use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, Styled, Window,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme, Sizable};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    rc::Rc,
    sync::Arc,
};

pub(super) struct KgAlternatives {
    sources: HashMap<SharedString, RoleSource>,
    input: Entity<InputState>,
    draft: Option<Draft>,
    tree: Option<Result<ConceptOutline, String>>,
}

struct ConceptOutline {
    focused_id: u64,
    label: String,
    groups: Vec<TreeGroup>,
    expanded: HashSet<String>,
}

struct Draft {
    record: KgRecord,
    cursor: usize,
}

impl KgAlternatives {
    pub(super) fn new(window: &mut Window, cx: &mut Context<TerraphimEditor>) -> Self {
        Self {
            sources: HashMap::new(),
            input: cx.new(|cx| InputState::new(window, cx).multi_line(true)),
            draft: None,
            tree: None,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.draft.is_some()
    }
}

impl TerraphimEditor {
    /// Associate roles with the exact backing file snapshots used for their loaded graphs.
    #[must_use]
    pub fn with_role_sources(mut self, sources: HashMap<SharedString, RoleSource>) -> Self {
        self.kg_alternatives.sources = sources;
        self
    }

    pub(super) fn install_role_completion(
        &self,
        role: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let provider = TerraphimCompletionProvider::with_defaults(
            Arc::clone(&self.roles[role].1),
            self.commands.clone(),
        );
        self.input_state.update(cx, |state, cx| {
            state.handle_action_for_context_menu(
                Box::new(gpui_component::input::Escape),
                window,
                cx,
            );
            state.lsp.completion_provider = Some(Rc::new(provider));
        });
    }

    pub(super) fn open_kg_alternatives(
        &mut self,
        range: &Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.kg_alternatives.is_open() {
            return true;
        }
        let role = &self.roles[self.active_role].0;
        let Some(graph) = self.role_graphs.get(role) else {
            return false;
        };
        let matches = graph.analyse(&self.document.body).matches;
        let covers_start = |term: &&terraphim_lsp_core::TermMatch| {
            term.range.start.utf16 <= range.start
                && range.start < term.range.end.utf16
                && range.end <= term.range.end.utf16
        };
        let term = matches.iter().find(covers_start).or_else(|| {
            range
                .is_empty()
                .then(|| {
                    matches
                        .iter()
                        .find(|term| term.range.end.utf16 == range.start)
                })
                .flatten()
        });
        let Some(term) = term else {
            return false;
        };

        let record = self
            .kg_alternatives
            .sources
            .get(role)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Load a role JSON or Markdown KG directory to change knowledge graph synonyms"
                )
            })
            .and_then(|source| source.record(term.concept_id));
        match record {
            Ok(record) => {
                self.inline_alternatives = None;
                self.document_status = None;
                self.kg_alternatives.input.update(cx, |input, cx| {
                    input.set_value(record.synonyms.join("\n"), window, cx);
                    input.focus(window, cx);
                });
                self.kg_alternatives.draft = Some(Draft {
                    record,
                    cursor: range.start,
                });
            }
            Err(error) => self.document_status = Some(error.to_string().into()),
        }
        cx.notify();
        true
    }

    fn save_kg_alternatives(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &self.kg_alternatives.draft else {
            return;
        };
        let role = self.roles[self.active_role].0.clone();
        let Some(source) = self.kg_alternatives.sources.get_mut(&role) else {
            return;
        };
        let mut record = draft.record.clone();
        record.synonyms = self
            .kg_alternatives
            .input
            .read(cx)
            .value()
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        match source.save(&record) {
            Ok((graph, index)) => {
                let saved_source = source.clone();
                for (name, role_index) in &mut self.roles {
                    if self
                        .kg_alternatives
                        .sources
                        .get(name)
                        .is_some_and(|candidate| candidate.path() == saved_source.path())
                    {
                        self.role_graphs.insert(name.clone(), graph.clone());
                        *role_index = Arc::clone(&index);
                        if *name != role {
                            self.kg_alternatives
                                .sources
                                .insert(name.clone(), saved_source.clone());
                        }
                    }
                }
                self.install_role_completion(self.active_role, window, cx);
                self.close_kg_alternatives(window, cx);
                self.document_status = Some("Saved knowledge graph synonyms".into());
            }
            Err(error) => self.document_status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn close_kg_alternatives(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(draft) = self.kg_alternatives.draft.take() {
            self.kg_alternatives.tree = None;
            self.focus_document(window, cx);
            self.focus_offset(draft.cursor, window, cx);
            self.document_status = None;
            cx.notify();
        }
    }

    fn toggle_concept_tree(&mut self, cx: &mut Context<Self>) {
        if self.kg_alternatives.tree.take().is_some() {
            cx.notify();
        } else if let Some(draft) = &self.kg_alternatives.draft {
            self.focus_concept_tree(draft.record.id, cx);
        }
    }

    fn focus_concept_tree(&mut self, id: u64, cx: &mut Context<Self>) {
        let role = &self.roles[self.active_role].0;
        let tree = self
            .kg_alternatives
            .sources
            .get(role)
            .ok_or_else(|| anyhow::anyhow!("Load a Markdown role to inspect concept relationships"))
            .and_then(|source| {
                Ok(ConceptOutline {
                    focused_id: id,
                    label: source.record(id)?.canonical,
                    groups: source.concept_tree(id)?,
                    expanded: HashSet::new(),
                })
            });
        self.kg_alternatives.tree = Some(tree.map_err(|error| error.to_string()));
        cx.notify();
    }

    #[cfg(feature = "gdp")]
    /// Inspect the visible concept outline through the test driver.
    #[must_use]
    pub fn headless_concept_tree(&self) -> serde_json::Value {
        match &self.kg_alternatives.tree {
            Some(Ok(tree)) => serde_json::json!({
                "concept_id": tree.focused_id.to_string(),
                "label": tree.label,
                "groups": tree.groups,
            }),
            Some(Err(error)) => serde_json::json!({"error": error}),
            None => serde_json::Value::Null,
        }
    }

    fn render_concept_tree(&self, cx: &Context<Self>) -> impl IntoElement {
        let view = div().flex().flex_col().gap_1();
        match &self.kg_alternatives.tree {
            Some(Ok(tree)) => view
                .child(
                    div()
                        .debug_selector(|| "concept-tree-heading".into())
                        .child(format!("Concept tree · {}", tree.label)),
                )
                .when_some(
                    self.kg_alternatives
                        .draft
                        .as_ref()
                        .map(|draft| draft.record.id)
                        .filter(|id| *id != tree.focused_id),
                    |view, id| {
                        view.child(
                            Button::new("kg-tree-back")
                                .debug_selector(|| "kg-tree-back".into())
                                .label("Back to original concept")
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |editor, _, _, cx| {
                                    editor.focus_concept_tree(id, cx);
                                })),
                        )
                    },
                )
                .when(tree.groups.is_empty(), |view| {
                    view.child("No declared relationships for this concept")
                })
                .child(tree_view::render(
                    "concept-tree",
                    &tree.groups,
                    &tree.expanded,
                    |editor, path, cx| {
                        if let Some(Ok(tree)) = &mut editor.kg_alternatives.tree {
                            tree_view::toggle(&mut tree.expanded, path);
                            cx.notify();
                        }
                    },
                    |editor, id, cx| {
                        if let Ok(id) = id.parse() {
                            editor.focus_concept_tree(id, cx);
                        }
                    },
                    cx,
                )),
            Some(Err(error)) => view.child(format!("Concept tree unavailable · {error}")),
            None => view,
        }
    }

    pub(super) fn render_kg_alternatives(&self, cx: &Context<Self>) -> impl IntoElement {
        let role = &self.roles[self.active_role].0;
        let name = self
            .kg_alternatives
            .draft
            .as_ref()
            .map_or("", |draft| draft.record.canonical.as_str());
        let path = self
            .kg_alternatives
            .sources
            .get(role)
            .and_then(|source| {
                self.kg_alternatives
                    .draft
                    .as_ref()
                    .and_then(|draft| source.record_path(draft.record.id).ok())
            })
            .map_or_else(String::new, |path| path.display().to_string());
        tree_view::keyboard_navigation(div(), "TerraphimKgAlternatives")
            .capture_action(cx.listener(|this, _: &super::Cancel, window, cx| {
                cx.stop_propagation();
                this.close_kg_alternatives(window, cx);
            }))
            .debug_selector(|| "kg-synonyms-panel".into())
            .flex()
            .flex_col()
            .gap_2()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(format!("{name} · {role}")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(path),
            )
            .child("Synonyms, one per line")
            .child(Input::new(&self.kg_alternatives.input).h(px(100.)))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("kg-save-synonyms")
                            .debug_selector(|| "kg-save-synonyms".into())
                            .label("Save synonyms")
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_kg_alternatives(window, cx);
                            })),
                    )
                    .child(
                        Button::new("kg-concept-tree")
                            .debug_selector(|| "kg-concept-tree".into())
                            .label(if self.kg_alternatives.tree.is_some() {
                                "Hide concept tree"
                            } else {
                                "Concept tree"
                            })
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_concept_tree(cx);
                            })),
                    )
                    .child(
                        Button::new("kg-cancel-synonyms")
                            .debug_selector(|| "kg-cancel-synonyms".into())
                            .label("Cancel")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_kg_alternatives(window, cx);
                            })),
                    ),
            )
            .when(self.kg_alternatives.tree.is_some(), |pane| {
                pane.child(self.render_concept_tree(cx))
            })
    }
}
