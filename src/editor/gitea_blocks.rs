use super::{document, tree_view, TerraphimEditor};
use crate::gitea_tasks::{CreateUncertain, GiteaTasks, IssueRef, IssueSnapshot, TaskTree};
use futures::{stream, StreamExt};
use gpui::prelude::FluentBuilder;
use gpui::{
    div, AppContext, ClipboardItem, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, SharedString, Styled, Window,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme, Disableable, Sizable};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

pub(super) struct TaskBlocks {
    provider: GiteaTasks,
    issues: HashMap<IssueRef, IssueCard>,
    pending: HashSet<IssueRef>,
    expanded: HashSet<IssueRef>,
    repository: Entity<InputState>,
    title: Entity<InputState>,
    body: Entity<InputState>,
    draft: Option<IssueDraft>,
    creating: bool,
    recovery: Option<IssueRef>,
    uncertain: Option<(String, String)>,
}

#[derive(Default)]
struct IssueCard {
    snapshot: Option<IssueSnapshot>,
    error: Option<String>,
    tree: TaskOutline,
}

#[derive(Default)]
struct TaskOutline {
    open: bool,
    pending: bool,
    snapshot: Option<TaskTree>,
    error: Option<String>,
    expanded: HashSet<String>,
}

struct IssueDraft {
    end: usize,
    generation: u64,
    document: String,
}

impl TaskBlocks {
    pub(super) fn new(window: &mut Window, cx: &mut Context<TerraphimEditor>) -> Self {
        Self {
            provider: GiteaTasks::from_env(),
            issues: HashMap::new(),
            pending: HashSet::new(),
            expanded: HashSet::new(),
            repository: cx.new(|cx| InputState::new(window, cx).placeholder("owner/repository")),
            title: cx.new(|cx| InputState::new(window, cx).placeholder("Issue title")),
            body: cx.new(|cx| InputState::new(window, cx).multi_line(true)),
            draft: None,
            creating: false,
            recovery: None,
            uncertain: None,
        }
    }

    pub(super) fn has_form(&self) -> bool {
        self.draft.is_some() || self.creating || self.recovery.is_some() || self.uncertain.is_some()
    }

    pub(super) fn close_draft(&mut self) {
        self.draft = None;
    }

    pub(super) fn issue_url(&self, issue: &IssueRef) -> String {
        issue.url(self.provider.origin())
    }

    pub(super) fn issue_in_block(&self, block: &str) -> Option<IssueRef> {
        block
            .contains("://")
            .then(|| IssueRef::in_block(block, self.provider.origin()))
            .flatten()
    }
}

impl TerraphimEditor {
    pub(super) fn refresh_task_blocks(&mut self, force: bool, cx: &mut Context<Self>) {
        let issues: Vec<_> = crate::blocks::parse(&self.document.body)
            .iter()
            .filter(|block| !matches!(block.kind, "Code" | "HTML"))
            .filter_map(|block| {
                self.gitea
                    .issue_in_block(&self.document.body[block.range.clone()])
            })
            .filter(|issue| force || !self.gitea.issues.contains_key(issue))
            .collect();
        self.fetch_issues(issues, cx);
    }

    fn fetch_issues(&mut self, issues: Vec<IssueRef>, cx: &mut Context<Self>) {
        let issues: Vec<_> = issues
            .into_iter()
            .filter(|issue| self.gitea.pending.insert(issue.clone()))
            .collect();
        if issues.is_empty() {
            return;
        }

        let provider = self.gitea.provider.clone();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |editor, cx| {
            let mut results = stream::iter(issues)
                .map(|issue| {
                    let provider = provider.clone();
                    executor.spawn(async move {
                        let result = provider.view(&issue);
                        (issue, result)
                    })
                })
                .buffer_unordered(4);
            while let Some((issue, result)) = results.next().await {
                let still_open = editor.update(cx, |editor, cx| {
                    editor.gitea.pending.remove(&issue);
                    let card = editor.gitea.issues.entry(issue).or_default();
                    match result {
                        Ok(snapshot) => {
                            card.snapshot = Some(snapshot);
                            card.error = None;
                        }
                        Err(error) => card.error = Some(error.to_string()),
                    }
                    cx.notify();
                });
                if still_open.is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn toggle_task_tree(&mut self, issue: IssueRef, cx: &mut Context<Self>) {
        let tree = &mut self.gitea.issues.entry(issue.clone()).or_default().tree;
        tree.open = !tree.open;
        if tree.open {
            self.fetch_task_tree(issue, cx);
        }
        cx.notify();
    }

    fn fetch_task_tree(&mut self, issue: IssueRef, cx: &mut Context<Self>) {
        let tree = &mut self.gitea.issues.entry(issue.clone()).or_default().tree;
        if tree.pending || !tree.open {
            return;
        }
        tree.pending = true;
        let provider = self.gitea.provider.clone();
        let reference = issue.clone();
        let result = cx
            .background_executor()
            .spawn(async move { provider.tree(&reference) });
        cx.spawn(async move |editor, cx| {
            let result = result.await;
            let _ = editor.update(cx, |editor, cx| {
                let tree = &mut editor.gitea.issues.entry(issue).or_default().tree;
                tree.pending = false;
                match result {
                    Ok(response) => {
                        tree.snapshot = Some(response);
                        tree.expanded.clear();
                        tree.error = None;
                    }
                    Err(error) => tree.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn begin_issue_creation(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.gitea.creating || self.gitea.draft.is_some() || self.gitea.recovery.is_some() {
            return;
        }
        let Some(body) = self.document.body.get(range.clone()) else {
            return;
        };
        let title = body
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default();
        let title: String = title
            .trim_start_matches(['#', '>', '-', '*', ' '])
            .chars()
            .take(120)
            .collect();
        let repository = self.gitea.provider.default_repository().to_owned();
        self.gitea
            .repository
            .update(cx, |input, cx| input.set_value(repository, window, cx));
        self.gitea
            .title
            .update(cx, |input, cx| input.set_value(title, window, cx));
        self.gitea
            .body
            .update(cx, |input, cx| input.set_value(body.to_owned(), window, cx));
        self.gitea
            .repository
            .update(cx, |input, cx| input.focus(window, cx));
        self.gitea.draft = Some(IssueDraft {
            end: range.end,
            generation: self.document_generation,
            document: self.document.body.clone(),
        });
        self.document_status = None;
        cx.notify();
    }

    fn create_issue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.gitea.creating || self.gitea.recovery.is_some() || self.gitea.uncertain.is_some() {
            return;
        }
        let Some(draft) = self.gitea.draft.take() else {
            return;
        };
        let repository = self.gitea.repository.read(cx).value().trim().to_owned();
        let title = self.gitea.title.read(cx).value().trim().to_owned();
        let body = self.gitea.body.read(cx).value().to_string();
        let valid_repo = repository.split_once('/').is_some_and(|(owner, repo)| {
            !owner.is_empty() && !repo.is_empty() && !repo.contains('/')
        });
        if !valid_repo || title.is_empty() {
            self.gitea.draft = Some(draft);
            self.document_status =
                Some("Enter a repository as owner/repository and an issue title".into());
            cx.notify();
            return;
        }

        self.blocks_focus.focus(window);
        self.gitea.creating = true;
        self.document_status = Some("Creating Gitea issue…".into());
        let provider = self.gitea.provider.clone();
        let submitted_repo = repository.clone();
        let request = cx
            .background_executor()
            .spawn(async move { provider.create(&repository, &title, &body) });
        cx.spawn_in(window, async move |editor, cx| {
            let result = request.await;
            let _ = editor.update_in(cx, |editor, window, cx| {
                editor.gitea.creating = false;
                match result {
                    Ok(issue) => editor.attach_created_issue(issue, &draft, window, cx),
                    Err(error) => {
                        if editor.document_generation == draft.generation {
                            editor.gitea.draft = Some(draft);
                        }
                        if error.is::<CreateUncertain>() {
                            editor.gitea.uncertain = Some((submitted_repo, error.to_string()));
                        }
                        editor.document_status = Some(error.to_string().into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn attach_created_issue(
        &mut self,
        issue: IssueRef,
        draft: &IssueDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = issue.url(self.gitea.provider.origin());
        let mut document = self.document.clone();
        let attached = self.document_generation == draft.generation
            && self.document.body == draft.document
            && self.block_draft.is_none()
            && document::replace_region(
                &mut document,
                draft.end..draft.end,
                &issue_link(&draft.document[draft.end..], &draft.document, &url),
            )
            .is_ok();
        if attached {
            self.commit_document(document, window, cx);
            self.document_status = Some(format!("Created Gitea issue #{}", issue.number).into());
        } else {
            self.gitea.recovery = Some(issue.clone());
            self.document_status = Some("Created the issue while the document was changing".into());
        }
        self.fetch_issues(vec![issue], cx);
    }

    pub(super) fn render_task_form(&self, cx: &Context<Self>) -> impl IntoElement {
        let recovery = self
            .gitea
            .recovery
            .as_ref()
            .map(|issue| issue.url(self.gitea.provider.origin()));
        div()
            .when(self.gitea.creating, |form| {
                form.child("Creating Gitea issue…")
            })
            .when_some(
                self.gitea.uncertain.clone(),
                |form, (repository, message)| {
                    let url = format!("{}/{repository}/issues", self.gitea.provider.origin());
                    form.child(
                        div().flex().flex_col().gap_2().child(message).child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("check-gitea-issues")
                                        .label("Check Gitea issues")
                                        .small()
                                        .on_click(
                                            cx.listener(move |_, _, _, cx| cx.open_url(&url)),
                                        ),
                                )
                                .child(
                                    Button::new("acknowledge-gitea-check")
                                        .label("I checked Gitea")
                                        .small()
                                        .ghost()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.gitea.uncertain = None;
                                            this.gitea.close_draft();
                                            this.document_status = None;
                                            cx.notify();
                                        })),
                                ),
                        ),
                    )
                },
            )
            .when_some(recovery, |form, url| {
                form.child(
                    Button::new("copy-created-issue")
                        .label("Copy created issue link")
                        .small()
                        .outline()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                            this.gitea.recovery = None;
                            cx.notify();
                        })),
                )
            })
            .when(
                self.gitea.draft.is_some() && self.gitea.uncertain.is_none(),
                |form| form.child(self.render_issue_draft(cx)),
            )
    }

    fn render_issue_draft(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .child("Create Gitea issue")
            .child(
                div()
                    .debug_selector(|| "gitea-repository".into())
                    .child("Repository")
                    .child(Input::new(&self.gitea.repository)),
            )
            .child(
                div()
                    .debug_selector(|| "gitea-title".into())
                    .child("Title")
                    .child(Input::new(&self.gitea.title)),
            )
            .child(
                div()
                    .debug_selector(|| "gitea-description".into())
                    .child("Description")
                    .child(Input::new(&self.gitea.body)),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("submit-gitea-issue")
                            .debug_selector(|| "submit-gitea-issue".into())
                            .small()
                            .primary()
                            .label("Create issue")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_issue(window, cx);
                            })),
                    )
                    .child(
                        Button::new("cancel-gitea-issue")
                            .small()
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.gitea.close_draft();
                                this.blocks_focus.focus(window);
                                cx.notify();
                            })),
                    ),
            )
    }

    pub(super) fn render_block_task(
        &self,
        block: &crate::blocks::Block,
        index: usize,
        text: String,
        cx: &Context<Self>,
    ) -> (impl IntoElement, Option<Button>) {
        let issue = (!matches!(block.kind, "Code" | "HTML"))
            .then(|| self.gitea.issue_in_block(&text))
            .flatten();
        let content = div().flex().flex_col().gap_2();
        if let Some(issue) = issue {
            let url = self.gitea.issue_url(&issue);
            let standalone = text.trim() == url || text.trim() == format!("<{url}>");
            return (
                content
                    .when(!standalone, |content| content.child(text))
                    .child(self.render_issue_card(&issue, index, cx)),
                None,
            );
        }
        let action = (self.block_draft.is_none()
            && !matches!(block.kind, "Code" | "HTML" | "Rule"))
        .then(|| {
            let range = block.range.clone();
            Button::new(("create-gitea-issue", index))
                .debug_selector(|| format!("create-gitea-issue-{index}"))
                .label("Create Gitea issue")
                .small()
                .ghost()
                .disabled(self.gitea.has_form())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.begin_issue_creation(range.clone(), window, cx);
                }))
        });
        (content.child(text), action)
    }

    /// Inspect the issue cards in the visible Blocks view through the test driver.
    #[cfg(feature = "gdp")]
    #[must_use]
    pub fn headless_gitea_cards(&self) -> serde_json::Value {
        if !self.show_blocks {
            return serde_json::json!([]);
        }
        let cards: Vec<_> = crate::blocks::parse(&self.document.body)
            .iter()
            .enumerate()
            .filter(|(_, block)| !matches!(block.kind, "Code" | "HTML"))
            .filter_map(|(index, block)| {
                let issue = self
                    .gitea
                    .issue_in_block(&self.document.body[block.range.clone()])?;
                let card = self.gitea.issues.get(&issue);
                let snapshot = card.and_then(|card| card.snapshot.as_ref());
                let expanded = self.gitea.expanded.contains(&issue);
                Some(serde_json::json!({
                    "index": index,
                    "number": issue.number,
                    "url": self.gitea.issue_url(&issue),
                    "pending": self.gitea.pending.contains(&issue),
                    "expanded": expanded,
                    "error": card.and_then(|card| card.error.as_ref()),
                    "title": snapshot.map(|value| &value.title),
                    "state": snapshot.map(|value| &value.state),
                    "body": snapshot.filter(|_| expanded).map(|value| &value.body),
                    "labels": snapshot.map(|value| &value.labels),
                    "assignees": snapshot.map(|value| &value.assignees),
                    "stale": snapshot.map(|value| value.stale),
                    "tree_open": card.is_some_and(|card| card.tree.open),
                    "tree_pending": card.is_some_and(|card| card.tree.pending),
                    "tree_error": card.and_then(|card| card.tree.error.as_ref()),
                    "tree_groups": card.filter(|card| card.tree.open).and_then(|card| card.tree.snapshot.as_ref()).map(|snap| &snap.groups),
                }))
            })
            .collect();
        serde_json::json!(cards)
    }

    fn render_issue_card(
        &self,
        issue: &IssueRef,
        index: usize,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let card = self.gitea.issues.get(issue);
        let snapshot = card.and_then(|card| card.snapshot.as_ref());
        let expanded = self.gitea.expanded.contains(issue);
        let label: SharedString =
            format!("{}/{} #{}", issue.owner, issue.repo, issue.number).into();
        div()
            .id(("gitea-card", index))
            .debug_selector(|| format!("gitea-card-{index}"))
            .flex()
            .flex_col()
            .gap_1()
            .child(label)
            .when_some(snapshot, |card, snapshot| {
                let fetched_at: chrono::DateTime<chrono::Utc> = snapshot.fetched_at.into();
                card.child(
                    div()
                        .debug_selector(|| format!("gitea-title-{index}"))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(snapshot.title.clone()),
                )
                .child(format!(
                    "{}{}{}",
                    snapshot.state,
                    if snapshot.labels.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", snapshot.labels.join(", "))
                    },
                    if snapshot.assignees.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", snapshot.assignees.join(", "))
                    }
                ))
                .child(div().text_xs().child(format!(
                    "{} {}",
                    if snapshot.stale { "Cached" } else { "Checked" },
                    fetched_at.format("%Y-%m-%d %H:%M UTC")
                )))
                .when(expanded, |card| {
                    card.child(div().child(snapshot.body.clone()))
                })
            })
            .when_some(card.and_then(|card| card.error.clone()), |card, error| {
                let message = if snapshot.is_some() {
                    format!("Refresh failed · {error}")
                } else {
                    error
                };
                card.child(div().text_xs().child(message))
            })
            .child(self.render_issue_actions(issue, index, card, cx))
            .when_some(card.filter(|card| card.tree.open), |element, card| {
                element.child(Self::render_task_tree(issue, index, &card.tree, cx))
            })
    }

    fn render_issue_actions(
        &self,
        issue: &IssueRef,
        index: usize,
        card: Option<&IssueCard>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let has_snapshot = card.is_some_and(|card| card.snapshot.is_some());
        let pending = self.gitea.pending.contains(issue);
        let expanded = self.gitea.expanded.contains(issue);
        let url = issue.url(self.gitea.provider.origin());
        let issue = issue.clone();
        let refresh = issue.clone();
        let tree_issue = issue.clone();
        let tree_open = card.is_some_and(|card| card.tree.open);
        div()
            .flex()
            .gap_2()
            .when(has_snapshot, |row| {
                row.child(
                    Button::new(("gitea-details", index))
                        .debug_selector(|| format!("gitea-details-{index}"))
                        .small()
                        .ghost()
                        .label(if expanded {
                            "Hide details"
                        } else {
                            "Show details"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.gitea.expanded.remove(&issue) {
                                this.gitea.expanded.insert(issue.clone());
                            }
                            cx.notify();
                        })),
                )
            })
            .child(
                Button::new(("gitea-refresh", index))
                    .debug_selector(|| format!("gitea-refresh-{index}"))
                    .small()
                    .ghost()
                    .disabled(pending)
                    .label(if pending { "Refreshing…" } else { "Refresh" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.fetch_issues(vec![refresh.clone()], cx);
                        this.fetch_task_tree(refresh.clone(), cx);
                    })),
            )
            .child(
                Button::new(("gitea-task-tree", index))
                    .debug_selector(|| format!("gitea-task-tree-{index}"))
                    .small()
                    .ghost()
                    .label(if tree_open {
                        "Hide task tree"
                    } else {
                        "Task tree"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_task_tree(tree_issue.clone(), cx);
                    })),
            )
            .child(
                Button::new(("gitea-open", index))
                    .debug_selector(|| format!("gitea-open-{index}"))
                    .small()
                    .ghost()
                    .label("Open in Gitea")
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
            )
    }

    fn render_task_tree(
        issue: &IssueRef,
        index: usize,
        tree: &TaskOutline,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let for_toggle = issue.clone();
        let for_open = issue.clone();
        let message = if tree.pending {
            "Loading task tree…".to_owned()
        } else if let Some(error) = &tree.error {
            format!("Task tree unavailable · {error}")
        } else if tree
            .snapshot
            .as_ref()
            .is_some_and(|snap| snap.groups.is_empty())
        {
            "No links in the returned snapshot. Coverage is unverified.".to_owned()
        } else if tree.snapshot.as_ref().is_some_and(|snap| snap.cached) {
            "Cached Gitea snapshot · same repository only · coverage unverified".to_owned()
        } else {
            "Gitea snapshot · same repository only · coverage unverified".to_owned()
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().child(message))
            .when_some(tree.snapshot.as_ref(), |element, snapshot| {
                element.child(tree_view::render(
                    &format!("task-tree-{index}"),
                    &snapshot.groups,
                    &tree.expanded,
                    move |editor, path, cx| {
                        if let Some(card) = editor.gitea.issues.get_mut(&for_toggle) {
                            tree_view::toggle(&mut card.tree.expanded, path);
                            cx.notify();
                        }
                    },
                    move |editor, id, cx| {
                        if let Ok(number) = id.parse() {
                            let selected = IssueRef {
                                number,
                                ..for_open.clone()
                            };
                            cx.open_url(&editor.gitea.issue_url(&selected));
                        }
                    },
                    cx,
                ))
            })
    }
}

fn issue_link(rest: &str, body: &str, url: &str) -> String {
    let newline = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let mut link = format!("{newline}{newline}<{url}>");
    if !rest.trim().is_empty() {
        let empty_lines = rest
            .lines()
            .take_while(|line| line.trim().is_empty())
            .count();
        for _ in empty_lines..2 {
            link.push_str(newline);
        }
    }
    link
}

#[cfg(test)]
mod tests {
    use super::issue_link;

    #[test]
    fn issue_link_is_a_separate_markdown_block_and_preserves_line_endings() {
        let url = "https://gitea.test/team/repo/issues/18";
        assert_eq!(
            issue_link("\nnext", "# Heading\nnext", url),
            format!("\n\n<{url}>\n")
        );
        assert_eq!(
            issue_link("\r\n\r\nnext", "A\r\n\r\nnext", url),
            format!("\r\n\r\n<{url}>")
        );
        assert_eq!(issue_link("\n", "A\n", url), format!("\n\n<{url}>"));
    }
}
