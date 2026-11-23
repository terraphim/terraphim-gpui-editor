//! Terraphim Editor component
//!
//! Main editor view that combines the Input component with completion provider
//! and slash command support.

use crate::completion::{CompletionConfig, TerraphimCompletionProvider};
use crate::document::{self, History, Operation};
use crate::slash_command::{CommandOutput, EditorAction, SlashCommandRegistry};
use anyhow::{anyhow, Result as AnyhowResult};
use gpui::prelude::FluentBuilder;
use gpui::StatefulInteractiveElement;
use gpui::{
    actions, anchored, canvas, deferred, div, point, px, App, AppContext, Bounds, ClipboardItem,
    Context, Entity, EntityInputHandler, EventEmitter, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Pixels, Point, Render, Result, SharedString, Styled, Subscription,
    Window,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::menu::{DropdownMenu, PopupMenuItem};
use gpui_component::scroll::ScrollableElement;
use gpui_component::text::TextView;
use gpui_component::ActiveTheme;
use gpui_component::RopeExt;
use gpui_component::Sizable;
use std::rc::Rc;
use std::sync::Arc;
use std::{cell::Cell, collections::HashMap, ops::Range, path::PathBuf};
use terraphim_alternatives::Document;
use terraphim_automata::autocomplete::AutocompleteIndex;
use terraphim_lsp_core::KgEngine;
use terraphim_markdown_parser::NormalizedMarkdown;

mod context_menu;
mod gitea_blocks;
mod kg_alternatives;

// Actions for the editor
actions!(
    terraphim_editor,
    [
        ReopenCompletions,
        RotateCompletionForward,
        RotateCompletionBackward,
        ExecuteCommand,
        EditAlternatives,
        NextSynonym,
        PreviousSynonym,
        ToggleBlocksView,
        CommitBlockDraft,
        Cancel,
    ]
);

/// Main Terraphim Editor view
///
/// Provides a multi-line text editor with:
/// - Terraphim autocomplete from thesaurus
/// - Slash command support
/// - Tab/Shift+Tab completion rotation
pub struct TerraphimEditor {
    /// Input state for the text buffer
    input_state: Entity<InputState>,
    /// Slash command registry for execution
    commands: SlashCommandRegistry,
    roles: Vec<(SharedString, Arc<AutocompleteIndex>)>,
    active_role: usize,
    role_graphs: HashMap<SharedString, KgEngine>,
    document: Document,
    history: History,
    alternative_input: Entity<InputState>,
    document_menu: Option<context_menu::DocumentContextMenu>,
    kg_alternatives: kg_alternatives::KgAlternatives,
    inline_alternatives: Option<InlineAlternatives>,
    input_bounds: Rc<Cell<Bounds<Pixels>>>,
    document_status: Option<SharedString>,
    show_preview: bool,
    /// Title for the editor window
    title: SharedString,
    /// Whether to show the block cards
    show_blocks: bool,
    block_input: Entity<InputState>,
    blocks_focus: FocusHandle,
    block_draft: Option<BlockDraft>,
    gitea: gitea_blocks::TaskBlocks,
    document_generation: u64,
    /// Current file path (if loaded from disk)
    file_path: Option<PathBuf>,
    /// Subscriptions kept alive with the editor
    _subscriptions: Vec<Subscription>,
}

struct InlineAlternatives {
    range: Range<usize>,
    position: Point<Pixels>,
}

/// Emitted when the active editor role changes.
pub struct RoleChanged(pub usize);

impl EventEmitter<RoleChanged> for TerraphimEditor {}

struct BlockDraft {
    range: Range<usize>,
    original: String,
}

impl TerraphimEditor {
    /// Create a new Terraphim Editor
    ///
    /// # Arguments
    /// * `index` - Pre-built autocomplete index from `terraphim_automata`
    /// * `commands` - Registry of slash commands
    /// * `title` - Title to display
    /// * `window` - GPUI window reference
    /// * `cx` - GPUI context
    pub fn new(
        index: Arc<AutocompleteIndex>,
        commands: SlashCommandRegistry,
        title: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Clone commands for completion provider
        let commands_for_provider = commands.clone();
        let roles = vec![(
            SharedString::from(index.name().to_owned()),
            Arc::clone(&index),
        )];

        // Create input state with multi-line support and completion provider
        let input_state = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .multi_line(true)
                .placeholder("Start typing... Use / for commands, Tab to cycle");

            // Set completion provider via the lsp field
            state.lsp.completion_provider = Some(Rc::new(TerraphimCompletionProvider::new(
                index,
                commands_for_provider,
                CompletionConfig::default(),
            )));

            state
        });
        let alternative_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type an alternative"));

        let block_input = cx.new(|cx| InputState::new(window, cx).multi_line(true));

        // Subscribe to input events
        let mut subscriptions = vec![cx.subscribe_in(&input_state, window, {
            let input_state = input_state.clone();
            move |this, _, ev: &InputEvent, window, cx| {
                this.handle_input_event(&input_state, ev, window, cx);
            }
        })];

        subscriptions.push(cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() && this.show_blocks {
                this.refresh_task_blocks(true, cx);
            }
        }));
        let gitea = gitea_blocks::TaskBlocks::new(window, cx);
        let kg_alternatives = kg_alternatives::KgAlternatives::new(window, cx);

        Self {
            input_state,
            commands,
            roles,
            active_role: 0,
            role_graphs: HashMap::new(),
            document: Document::default(),
            history: History::default(),
            alternative_input,
            document_menu: None,
            kg_alternatives,
            inline_alternatives: None,
            input_bounds: Rc::default(),
            document_status: None,
            show_preview: false,
            title: title.into(),
            show_blocks: false,
            block_input,
            blocks_focus: cx.focus_handle(),
            block_draft: None,
            gitea,
            document_generation: 0,
            file_path: None,
            _subscriptions: subscriptions,
        }
    }

    /// Add autocomplete roles to the toolbar picker.
    #[must_use]
    pub fn with_roles(mut self, roles: Vec<(SharedString, Arc<AutocompleteIndex>)>) -> Self {
        self.roles.extend(roles);
        self
    }

    /// Associate role names with the same KG data used to build autocomplete.
    #[must_use]
    pub fn with_role_graphs(mut self, graphs: HashMap<SharedString, KgEngine>) -> Self {
        self.role_graphs = graphs;
        self
    }

    /// The role names in selector order.
    #[must_use]
    pub fn role_names(&self) -> Vec<SharedString> {
        self.roles.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Focus the main document input.
    pub fn focus_document(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input_state
            .update(cx, |input, cx| input.focus(window, cx));
    }

    /// Change the active role without replacing the open document.
    pub fn select_role(&mut self, role: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_role == role || role >= self.roles.len() {
            return;
        }

        if self.kg_alternatives.is_open() {
            self.document_status = Some("Save or cancel the knowledge graph edit first".into());
            cx.notify();
            return;
        }
        self.install_role_completion(role, window, cx);
        let name = &self.roles[role].0;
        self.active_role = role;
        self.inline_alternatives = None;
        log::info!("Switched autocomplete role to {name}");
        cx.emit(RoleChanged(role));
        cx.notify();
    }

    fn apply_normalized_markdown(
        &mut self,
        normalized: NormalizedMarkdown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.document.annotations.is_empty() {
            self.document.body = normalized.markdown;
        }
        self.refresh_input(window, cx);
    }

    fn refresh_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input_state.read(cx).value().as_ref() != self.document.body {
            self.input_state.update(cx, |state, cx| {
                state.set_value(self.document.body.clone(), window, cx);
            });
        }
        cx.notify();
    }

    fn sync_document(&mut self, cx: &App) -> AnyhowResult<()> {
        let text = self.input_state.read(cx).value();
        if text.as_ref() != self.document.body {
            let before = self.document.clone();
            document::sync(&mut self.document, text.to_string())?;
            self.history.record(before, &self.document);
            self.inline_alternatives = None;
        }
        Ok(())
    }

    fn normalize_and_refresh_blocks(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyhowResult<()> {
        self.sync_document(cx)?;
        let normalized = match terraphim_markdown_parser::normalize_markdown(&self.document.body) {
            Ok(normalized) => normalized,
            Err(err) => {
                self.document_status = Some(err.to_string().into());
                cx.notify();
                return Err(err.into());
            }
        };

        let before = self.document.clone();
        self.apply_normalized_markdown(normalized, window, cx);
        self.history.record(before, &self.document);
        Ok(())
    }

    fn toggle_blocks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = if self.show_blocks {
            self.finish_block(window, cx)
        } else {
            self.sync_document(cx)
        };
        match result {
            Ok(()) => {
                self.show_blocks = !self.show_blocks;
                if self.show_blocks {
                    self.blocks_focus.focus(window);
                    self.refresh_task_blocks(true, cx);
                } else {
                    self.gitea.close_draft();
                    self.focus_document(window, cx);
                }
            }
            Err(error) => self.document_status = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn begin_block(
        &mut self,
        block: &crate::blocks::Block,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let original = self
            .document
            .body
            .get(block.range.clone())
            .map(str::to_owned);
        if let Err(error) = self.finish_block(window, cx) {
            self.document_status = Some(error.to_string().into());
            cx.notify();
            return;
        }
        let Some(original) = original
            .filter(|text| self.document.body.get(block.range.clone()) == Some(text.as_str()))
        else {
            return;
        };
        self.block_input.update(cx, |input, cx| {
            input.set_value(original.clone(), window, cx);
            input.focus(window, cx);
        });
        self.block_draft = Some(BlockDraft {
            range: block.range.clone(),
            original,
        });
        self.document_status = None;
        cx.notify();
    }

    fn add_block(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let end = self.document.body.len();
        self.begin_block(
            &crate::blocks::Block {
                range: end..end,
                move_start: end,
                kind: "Paragraph",
            },
            window,
            cx,
        );
    }

    fn change_blocks(
        &mut self,
        edit: impl FnOnce(&mut Document) -> AnyhowResult<()>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut document = self.document.clone();
        match edit(&mut document) {
            Ok(()) => self.commit_document(document, window, cx),
            Err(error) => {
                self.document_status = Some(error.to_string().into());
                cx.notify();
            }
        }
    }

    fn finish_block(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyhowResult<()> {
        let Some(draft) = self.block_draft.as_ref() else {
            return Ok(());
        };
        anyhow::ensure!(
            self.document.body.get(draft.range.clone()) == Some(draft.original.as_str()),
            "Block changed, draft is still open"
        );
        let mut candidate = self.document.clone();
        let mut edited = self.block_input.read(cx).value().to_string();
        if draft.range.is_empty()
            && !edited.trim().is_empty()
            && !candidate.body.is_empty()
            && !candidate.body.ends_with("\n\n")
        {
            let separator = if candidate.body.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            edited.insert_str(0, separator);
        }
        document::replace_region(&mut candidate, draft.range.clone(), &edited)?;
        self.block_draft = None;
        self.commit_document(candidate, window, cx);
        if self.show_blocks {
            self.blocks_focus.focus(window);
        }
        Ok(())
    }

    fn apply_block(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.finish_block(window, cx) {
            self.document_status = Some(error.to_string().into());
            cx.notify();
        }
    }

    /// Handle input events
    fn handle_input_event(
        &mut self,
        input_state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                if let Err(error) = self.sync_document(cx) {
                    self.document_status = Some(error.to_string().into());
                }
                cx.notify();
            }
            InputEvent::PressEnter { secondary } => {
                // Enter pressed - check for slash command execution
                if !*secondary {
                    self.try_execute_slash_command(input_state, window, cx);
                }
            }
            InputEvent::Focus => {
                log::debug!("Input focused");
            }
            InputEvent::Blur => {
                log::debug!("Input blurred");
            }
        }
    }

    fn complete_slash_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(selection) = self.selection(window, cx).filter(Range::is_empty) else {
            return false;
        };
        if self.sync_document(cx).is_err() {
            return false;
        }
        let Some(cursor) =
            terraphim_alternatives::utf16_to_byte(&self.document.body, selection.start)
        else {
            return false;
        };
        let body = &self.document.body;
        let line_start = body[..cursor].rfind('\n').map_or(0, |index| index + 1);
        let line_end = body[cursor..]
            .find('\n')
            .map_or(body.len(), |index| cursor + index);
        let token = body[line_start..cursor].trim_end();
        if token.chars().any(char::is_whitespace) || !body[cursor..line_end].trim().is_empty() {
            return false;
        }
        let command = match token {
            "se" | "/se" => self.commands.get("search"),
            _ => token
                .strip_prefix('/')
                .and_then(|query| self.commands.complete_duplicated_prefix(query)),
        };
        let Some(command) = command else {
            return false;
        };
        let replacement = if command.accepts_arguments() {
            format!("/{} ", command.name())
        } else {
            format!("/{}", command.name())
        };
        let offset = body[..line_start].encode_utf16().count() + replacement.encode_utf16().count();
        let mut candidate = self.document.clone();
        if document::replace_region(&mut candidate, line_start..line_end, &replacement).is_err() {
            return false;
        }
        self.commit_document(candidate, window, cx);
        self.focus_offset(offset, window, cx);
        true
    }

    fn accept_document_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.complete_slash_shortcut(window, cx)
            || self.input_state.update(cx, |input, cx| {
                input.handle_action_for_context_menu(
                    Box::new(gpui_component::input::Enter { secondary: false }),
                    window,
                    cx,
                )
            })
    }

    /// Try to execute a slash command from the current line
    fn try_execute_slash_command(
        &mut self,
        input_state: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = input_state.read(cx).value().to_string();

        // Find the line that was just entered (look for last complete line before cursor)
        // For simplicity, check if any line starts with / and execute it
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('/') {
                if let Some(output) = self.execute_command_line(trimmed, window, cx) {
                    self.handle_command_output(trimmed, output, window, cx);
                    return;
                }
            }
        }
    }

    /// Parse and execute a command line (e.g., "/help" or "/heading 2 Title")
    fn execute_command_line(
        &self,
        line: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<CommandOutput> {
        // Parse: /command arg1 arg2 ...
        let without_slash = line.strip_prefix('/')?;
        let mut parts = without_slash.split_whitespace();
        let command_name = parts.next()?;
        let args: Vec<String> = parts.map(String::from).collect();

        // Find and execute command
        if let Some(cmd) = self.commands.get(command_name) {
            log::info!("Executing slash command: /{command_name} {args:?}");
            Some(cmd.execute(&args, window, cx))
        } else {
            log::warn!("Unknown slash command: /{command_name}");
            None
        }
    }

    /// Handle command output by replacing the command line with output
    fn handle_command_output(
        &mut self,
        command_line: &str,
        output: CommandOutput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(action) = output.editor_action {
            match action {
                EditorAction::NormalizeIds => {
                    let _ = self.normalize_and_refresh_blocks(window, cx);
                }
                EditorAction::ToggleBlocks => {
                    self.toggle_blocks(window, cx);
                }
                EditorAction::OpenFile(path) => {
                    if let Err(err) = self.open_file(PathBuf::from(path), window, cx) {
                        log::error!("Failed to open file: {err}");
                        self.document_status =
                            Some(format!("Could not open Markdown: {err}").into());
                        cx.notify();
                    }
                    return;
                }
                EditorAction::SaveFile(path) => {
                    let path = path.map(PathBuf::from);
                    let result = self.sync_document(cx).and_then(|()| {
                        let mut document = self.document.clone();
                        document::replace_command(&mut document, command_line, "")?;
                        self.save_document(document, path, window, cx)
                    });
                    match result {
                        Ok(saved) => {
                            log::info!("Saved markdown to {}", saved.display());
                            self.document_status = Some("Saved Markdown".into());
                        }
                        Err(err) => {
                            log::error!("Failed to save markdown: {err}");
                            self.document_status =
                                Some(format!("Could not save Markdown: {err}").into());
                        }
                    }
                    cx.notify();
                    return;
                }
            }
        }

        // If there's text to insert, replace the command line
        if let Some(insert_text) = output.insert_text {
            let result = self.sync_document(cx).and_then(|()| {
                let mut document = self.document.clone();
                document::replace_command(&mut document, command_line, &insert_text)?;
                self.commit_document(document, window, cx);
                Ok(())
            });
            match result {
                Ok(()) => {}
                Err(error) => {
                    self.document_status = Some(error.to_string().into());
                    cx.notify();
                }
            }
        }

        // Log any message
        if let Some(msg) = output.message {
            log::info!("Command output: {msg}");
        }
    }

    /// Get the current text content
    pub fn text(&self, cx: &App) -> String {
        self.input_state.read(cx).value().to_string()
    }

    /// Set the text content (normalizes and refreshes blocks)
    pub fn set_text(
        &mut self,
        text: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw = text.into().to_string();
        if let Err(err) = self.load_markdown(&raw, window, cx) {
            self.document_status = Some(err.to_string().into());
            cx.notify();
        }
    }

    /// Load markdown, normalize IDs, and refresh block index.
    ///
    /// # Errors
    /// Returns an error if the Markdown cannot be parsed or normalized.
    pub fn load_markdown(
        &mut self,
        markdown: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), terraphim_markdown_parser::MarkdownParserError> {
        let document = document::open(markdown).map_err(|error| {
            terraphim_markdown_parser::MarkdownParserError::Markdown(error.to_string())
        })?;
        let normalized = terraphim_markdown_parser::normalize_markdown(&document.body)?;
        self.document = document;
        self.document_generation = self.document_generation.wrapping_add(1);
        self.gitea.close_draft();
        self.history = History::default();
        self.inline_alternatives = None;
        self.document_status = None;
        self.apply_normalized_markdown(normalized, window, cx);
        if self.show_blocks {
            self.refresh_task_blocks(true, cx);
        }
        Ok(())
    }

    /// Open a Markdown file from disk.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or its Markdown cannot be normalized.
    pub fn open_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyhowResult<()> {
        let content = std::fs::read_to_string(&path)
            .map_err(|err| anyhow!("Failed to read {}: {}", path.display(), err))?;
        self.finish_block(window, cx)?;
        self.load_markdown(&content, window, cx)?;
        self.title = format!("Terraphim Editor — {}", path.display()).into();
        self.file_path = Some(path);
        cx.notify();
        Ok(())
    }

    /// Save the current Markdown to disk with normalized block IDs.
    ///
    /// # Errors
    /// Returns an error if the text cannot be normalized, no path is available,
    /// or the file cannot be written.
    pub fn save_file(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyhowResult<PathBuf> {
        self.sync_document(cx)?;
        self.finish_block(window, cx)?;
        self.save_document(self.document.clone(), path, window, cx)
    }

    fn save_document(
        &mut self,
        mut document: Document,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyhowResult<PathBuf> {
        let normalized = terraphim_markdown_parser::normalize_markdown(&document.body)?;
        let path = path
            .or_else(|| self.file_path.clone())
            .ok_or_else(|| anyhow!("No file path set. Use /save <path>"))?;
        if document.annotations.is_empty() {
            document.body.clone_from(&normalized.markdown);
        }
        let source = terraphim_alternatives::write(&document);
        std::fs::write(&path, source)
            .map_err(|err| anyhow!("Failed to write {}: {}", path.display(), err))?;
        self.file_path = Some(path.clone());
        self.commit_document(document, window, cx);
        self.apply_normalized_markdown(normalized, window, cx);
        Ok(path)
    }

    /// Return normalized Markdown for saving.
    ///
    /// # Errors
    /// Returns an error if the text cannot be normalized.
    pub fn normalized_text(
        &self,
        cx: &App,
    ) -> Result<String, terraphim_markdown_parser::MarkdownParserError> {
        let mut document = self.document.clone();
        document::sync(&mut document, self.text(cx)).map_err(|error| {
            terraphim_markdown_parser::MarkdownParserError::Markdown(error.to_string())
        })?;
        if document.annotations.is_empty() {
            document.body = terraphim_markdown_parser::ensure_terraphim_block_ids(&document.body)?;
        }
        Ok(terraphim_alternatives::write(&document))
    }

    fn apply_document_operation(
        &mut self,
        operation: Operation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyhowResult<()> {
        self.sync_document(cx)?;
        let range = self
            .input_state
            .update(cx, |input, cx| {
                input
                    .selected_text_range(true, window, cx)
                    .map(|selection| selection.range)
            })
            .ok_or_else(|| anyhow!("Select text in the editor first"))?;
        let alternative = if matches!(operation, Operation::AddAlternative) {
            cx.read_from_clipboard().and_then(|item| item.text())
        } else {
            None
        };
        let mut document = self.document.clone();
        operation.apply(&mut document, range, alternative)?;
        self.commit_document(document, window, cx);
        Ok(())
    }

    fn commit_document(&mut self, document: Document, window: &mut Window, cx: &mut Context<Self>) {
        let before = std::mem::replace(&mut self.document, document);
        self.history.record(before, &self.document);
        self.inline_alternatives = None;
        self.document_status = None;
        self.refresh_input(window, cx);
        if self.show_blocks {
            self.refresh_task_blocks(false, cx);
        }
    }

    fn travel_history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.sync_document(cx).is_err() {
            if redo {
                return;
            }
        } else if !self.history.travel(&mut self.document, redo) {
            return;
        }
        self.inline_alternatives = None;
        self.document_status = None;
        self.refresh_input(window, cx);
    }

    fn selection(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.input_state.update(cx, |input, cx| {
            input
                .selected_text_range(true, window, cx)
                .map(|selection| selection.range)
        })
    }

    fn focus_offset(&self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(byte) = terraphim_alternatives::utf16_to_byte(&self.document.body, offset) {
            self.input_state.update(cx, |input, cx| {
                let position = input.text().offset_to_position(byte);
                input.set_cursor_position(position, window, cx);
            });
        }
    }

    fn cycle_at_cursor(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.sync_document(cx).and_then(|()| {
            let range = self
                .selection(window, cx)
                .ok_or_else(|| anyhow!("No editor selection"))?;
            let mut candidate = self.document.clone();
            let offset = if let Some(span) = document::span_at(&candidate, &range) {
                let id = span.id.clone();
                Operation::Cycle(if forward { 1 } else { -1 }).apply(
                    &mut candidate,
                    range.clone(),
                    None,
                )?;
                candidate
                    .span(&id)
                    .map_or(range.start, |span| span.anchor.start)
            } else {
                let name = &self.roles[self.active_role].0;
                let engine = self
                    .role_graphs
                    .get(name)
                    .ok_or_else(|| anyhow!("This role has no knowledge graph"))?;
                document::cycle_kg(&mut candidate, engine, range.start, forward)?
            };
            self.commit_document(candidate, window, cx);
            self.focus_offset(offset, window, cx);
            Ok(())
        });
        if let Err(error) = result {
            self.document_status = Some(error.to_string().into());
            cx.notify();
        }
    }

    fn open_inline_alternatives(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(range) = self.selection(window, cx) else {
            return;
        };
        if self.sync_document(cx).is_err() {
            return;
        }
        if self.open_kg_alternatives(&range, window, cx) {
            return;
        }
        if range.is_empty() && document::span_at(&self.document, &range).is_none() {
            self.document_status = Some("Select text to add an alternative".into());
            cx.notify();
            return;
        }
        let bounds = self.input_bounds.get();
        let position = self.input_state.update(cx, |input, cx| {
            input
                .bounds_for_range(range.start..range.start, bounds, window, cx)
                .map_or(bounds.origin, |selection| selection.bottom_left())
        });
        self.inline_alternatives = Some(InlineAlternatives { range, position });
        self.alternative_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn submit_inline(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.sync_document(cx).and_then(|()| {
            let inline = self
                .inline_alternatives
                .as_ref()
                .ok_or_else(|| anyhow!("The selection changed"))?;
            let range = inline.range.clone();
            let mut candidate = self.document.clone();
            let id = document::span_at(&candidate, &range).map(|span| span.id.clone());
            if let Some(index) = index {
                let id = id
                    .as_deref()
                    .ok_or_else(|| anyhow!("The selection changed"))?;
                candidate.set_active(id, index)?;
            } else {
                let alternative = self.alternative_input.read(cx).value().to_string();
                Operation::AddAlternative.apply(
                    &mut candidate,
                    range.clone(),
                    Some(alternative),
                )?;
            }
            self.commit_document(candidate, window, cx);
            self.focus_offset(range.start, window, cx);
            Ok(())
        });
        if let Err(error) = result {
            self.document_status = Some(error.to_string().into());
            cx.notify();
        }
    }

    fn copy_clean_markdown(&mut self, cx: &mut Context<Self>) -> AnyhowResult<()> {
        self.sync_document(cx)?;
        cx.write_to_clipboard(ClipboardItem::new_string(self.document.export()));
        self.document_status = Some("Copied clean Markdown".into());
        Ok(())
    }

    fn render_inline_alternatives(
        &self,
        inline: &InlineAlternatives,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let rows = document::span_at(&self.document, &inline.range).map(|span| {
            span.alts.iter().enumerate().map(|(index, alternative)| {
                Button::new(("inline-alternative", index))
                    .label(if index == span.active {
                        format!("✓ {}", alternative.text)
                    } else {
                        alternative.text.clone()
                    })
                    .small()
                    .ghost()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.submit_inline(Some(index), window, cx);
                    }))
            })
        });

        deferred(
            anchored()
                .position(inline.position + point(px(0.), px(4.)))
                .snap_to_window()
                .child(
                    div()
                        .id("inline-alternatives")
                        .w(px(330.))
                        .p_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .shadow_md()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.inline_alternatives = None;
                            cx.notify();
                        }))
                        .capture_action(cx.listener(
                            |this, _: &gpui_component::input::Escape, window, cx| {
                                cx.stop_propagation();
                                if let Some(inline) = this.inline_alternatives.take() {
                                    this.focus_offset(inline.range.start, window, cx);
                                }
                                cx.notify();
                            },
                        ))
                        .capture_action(cx.listener(
                            |this, _: &gpui_component::input::Enter, window, cx| {
                                cx.stop_propagation();
                                this.submit_inline(None, window, cx);
                            },
                        ))
                        .child(div().text_sm().child("Alternatives"))
                        .when_some(rows, |panel, rows| {
                            panel.child(
                                div()
                                    .max_h(px(180.))
                                    .overflow_y_scrollbar()
                                    .flex()
                                    .flex_col()
                                    .children(rows),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(Input::new(&self.alternative_input).small())
                                .child(
                                    Button::new("add-inline-alternative")
                                        .debug_selector(|| "add-inline-alternative".into())
                                        .label("Add")
                                        .small()
                                        .primary()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.submit_inline(None, window, cx);
                                        })),
                                ),
                        ),
                ),
        )
    }

    fn render_annotations_menu(cx: &Context<Self>) -> impl IntoElement {
        let document_editor = cx.entity().downgrade();
        Button::new("document-annotations")
            .label("Annotations")
            .small()
            .outline()
            .dropdown_menu(move |menu, _, cx| {
                let has_clipboard = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .is_some();
                Self::annotation_items(menu, document_editor.clone(), has_clipboard)
            })
    }

    /// Render the toolbar
    fn render_toolbar(&self, _window: &mut Window, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let editor = cx.entity().downgrade();
        let active_role = self.active_role;
        let role_names: Vec<_> = self.roles.iter().map(|(name, _)| name.clone()).collect();

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .truncate()
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Self::render_annotations_menu(cx))
                    .child(
                        Button::new("autocomplete-role")
                            .label(format!("Role: {}", role_names[active_role]))
                            .small()
                            .outline()
                            .dropdown_menu(move |mut menu, _, _| {
                                for (role, name) in role_names.iter().enumerate() {
                                    let editor = editor.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(name.clone())
                                            .checked(role == active_role)
                                            .on_click(move |_, window, cx| {
                                                _ = editor.update(cx, |editor, cx| {
                                                    editor.select_role(role, window, cx);
                                                });
                                            }),
                                    );
                                }
                                menu
                            }),
                    )
                    .child(
                        Button::new("markdown-preview")
                            .debug_selector(|| "markdown-preview".into())
                            .label(if self.show_preview {
                                "Preview ✓"
                            } else {
                                "Preview"
                            })
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_preview = !this.show_preview;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("terraphim-ids-btn")
                            .debug_selector(|| "terraphim-ids-btn".into())
                            .label("IDs")
                            .small()
                            .ghost()
                            .tooltip("Insert missing Terraphim block IDs")
                            .on_click(cx.listener(|this, _ev, window, cx| {
                                if this.document.annotations.is_empty() {
                                    let _ = this.normalize_and_refresh_blocks(window, cx);
                                } else {
                                    this.document_status = Some(
                                        "Block IDs are kept as-is in annotated documents".into(),
                                    );
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("terraphim-blocks-btn")
                            .debug_selector(|| "terraphim-blocks-btn".into())
                            .label(if self.show_blocks { "Text" } else { "Blocks" })
                            .small()
                            .ghost()
                            .tooltip("Switch between text and editable Markdown blocks")
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.toggle_blocks(window, cx);
                            })),
                    ),
            )
    }

    fn render_block_draft(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .key_context("TerraphimBlockDraft")
            .on_action(cx.listener(|this, _: &CommitBlockDraft, window, cx| {
                this.apply_block(window, cx);
            }))
            .capture_action(cx.listener(
                |this, action: &gpui_component::input::Enter, window, cx| {
                    if action.secondary {
                        cx.stop_propagation();
                        this.apply_block(window, cx);
                    }
                },
            ))
            .child(Input::new(&self.block_input).h(px(110.)))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("apply-block")
                            .debug_selector(|| "apply-block".into())
                            .label("Apply")
                            .small()
                            .primary()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.apply_block(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("cancel-block")
                            .debug_selector(|| "cancel-block".into())
                            .label("Cancel")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.blocks_focus.focus(window);
                                this.block_draft = None;
                                this.document_status = None;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn render_blocks(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let cards = crate::blocks::parse(&self.document.body);
        let draft = self.block_draft.as_ref();
        div()
            .id("blocks-view")
            .track_focus(&self.blocks_focus)
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .when(self.gitea.has_form(), |list| {
                list.child(self.render_task_form(cx))
            })
            .children(cards.iter().enumerate().map(|(index, block)| {
                let text = self.document.body[block.range.clone()].to_owned();
                let card = div()
                    .id(("block-card", index))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.secondary)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(block.kind),
                    );
                if draft.is_some_and(|draft| draft.range == block.range) {
                    return card.child(self.render_block_draft(cx));
                }
                let edit_block = block.clone();
                let mut actions = div().flex().gap_2().child(
                    Button::new(("edit-block", index))
                        .debug_selector(|| format!("edit-block-{index}"))
                        .label("Edit")
                        .small()
                        .outline()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.begin_block(&edit_block, window, cx);
                        })),
                );
                if draft.is_none() {
                    let range = block.move_start..block.range.end;
                    actions = actions.child(
                        Button::new(("delete-block", index))
                            .label("Delete")
                            .small()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.change_blocks(
                                    |doc| document::replace_region(doc, range.clone(), ""),
                                    window,
                                    cx,
                                );
                            })),
                    );
                    for (label, neighbor) in [
                        ("Move up", index.checked_sub(1).and_then(|i| cards.get(i))),
                        ("Move down", cards.get(index + 1)),
                    ] {
                        if let Some(neighbor) = neighbor {
                            let (first, second) = if neighbor.range.start < block.range.start {
                                (neighbor.clone(), block.clone())
                            } else {
                                (block.clone(), neighbor.clone())
                            };
                            actions = actions.child(
                                Button::new((label, index))
                                    .label(label)
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.change_blocks(
                                            |doc| {
                                                crate::blocks::swap_adjacent(doc, &first, &second)
                                            },
                                            window,
                                            cx,
                                        );
                                    })),
                            );
                        }
                    }
                }
                let (content, task_action) = self.render_block_task(block, index, text, cx);
                card.child(content).child(actions.children(task_action))
            }))
            .when(draft.is_some_and(|draft| draft.range.is_empty()), |list| {
                list.child(self.render_block_draft(cx))
            })
            .when(draft.is_none(), |list| {
                list.child(Self::render_block_toolbar(cx))
            })
    }

    fn render_block_toolbar(cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .gap_2()
            .child(
                Button::new("add-block")
                    .label("Add block")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| this.add_block(window, cx))),
            )
            .child(
                Button::new("undo-block")
                    .debug_selector(|| "undo-block".into())
                    .label("Undo")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.travel_history(false, window, cx);
                    })),
            )
            .child(
                Button::new("redo-block")
                    .label("Redo")
                    .small()
                    .ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.travel_history(true, window, cx);
                    })),
            )
    }

    /// Render the status bar
    fn render_status_bar(&self, _window: &mut Window, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let counts = self.document.counts();

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_end()
            .gap_4()
            .px_4()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .when_some(self.document_status.clone(), |row, status| {
                row.child(div().flex_1().text_xs().child(status))
            })
            .when(!self.document.annotations.is_empty(), |row| {
                row.child(div().text_xs().child(format!(
                    "{} alternative spans · {} ghosts · {} overflow words",
                    self.document.annotations.spans.len(),
                    self.document.annotations.ghosts.len(),
                    terraphim_alternatives::words::count_words(&self.document.annotations.overflow),
                )))
            })
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} chars", counts.chars)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} words", counts.words)),
            )
    }
}

impl TerraphimEditor {
    fn render_preview(&self, window: &mut Window, cx: &mut App) -> TextView {
        use std::hash::{DefaultHasher, Hash as _, Hasher as _};

        // TextView parses fenced code with the theme captured when its state is created.
        let mut hasher = DefaultHasher::new();
        cx.theme().highlight_theme.hash(&mut hasher);
        TextView::markdown(
            ("clean-markdown-preview", hasher.finish()),
            self.document.export(),
            window,
            cx,
        )
    }
}

impl TerraphimEditor {
    fn render_document(
        &self,
        editor_font: impl Into<SharedString>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let input_bounds = self.input_bounds.clone();
        div()
            .relative()
            .flex_1()
            .overflow_hidden()
            .key_context("TerraphimDocument")
            .debug_selector(|| "document-context-target".into())
            .capture_any_mouse_down(cx.listener(Self::open_document_context_menu))
            .children(
                self.document_menu
                    .as_ref()
                    .map(context_menu::DocumentContextMenu::render),
            )
            .capture_action(
                cx.listener(|this, _: &gpui_component::input::Undo, window, cx| {
                    cx.stop_propagation();
                    this.travel_history(false, window, cx);
                }),
            )
            .capture_action(
                cx.listener(|this, _: &gpui_component::input::Redo, window, cx| {
                    cx.stop_propagation();
                    this.travel_history(true, window, cx);
                }),
            )
            .capture_action(cx.listener(
                |this, action: &gpui_component::input::Enter, window, cx| {
                    if !action.secondary && this.accept_document_completion(window, cx) {
                        cx.stop_propagation();
                    }
                },
            ))
            .capture_action(cx.listener(
                |this, _: &gpui_component::input::IndentInline, window, cx| {
                    if this.accept_document_completion(window, cx) {
                        cx.stop_propagation();
                    }
                },
            ))
            .on_action(cx.listener(|this, _: &EditAlternatives, window, cx| {
                this.open_inline_alternatives(window, cx);
            }))
            .on_action(cx.listener(|this, _: &NextSynonym, window, cx| {
                this.cycle_at_cursor(true, window, cx);
            }))
            .on_action(cx.listener(|this, _: &PreviousSynonym, window, cx| {
                this.cycle_at_cursor(false, window, cx);
            }))
            .child(
                canvas(
                    move |bounds, _, _| input_bounds.set(bounds),
                    |_, (), _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                Input::new(&self.input_state)
                    .h_full()
                    .text_base()
                    .font_family(editor_font),
            )
            .child(crate::indicators::render(
                self.input_state.clone(),
                &self.document,
                self.role_graphs.get(&self.roles[self.active_role].0),
            ))
    }
}

impl Render for TerraphimEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(feature = "gdp")]
        if self.show_blocks {
            crate::indicators::clear_snapshot(cx);
        }
        let background = cx.theme().background;
        let border = cx.theme().border;
        let preview = self.show_preview.then(|| self.render_preview(window, cx));
        #[cfg(target_os = "linux")]
        let editor_font = cx.theme().mono_font_family.clone();
        #[cfg(not(target_os = "linux"))]
        let editor_font = "Berkeley Mono, SF Mono, Menlo, monospace";

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .on_action(cx.listener(|this, _: &ToggleBlocksView, window, cx| {
                this.toggle_blocks(window, cx);
            }))
            // Toolbar
            .child(self.render_toolbar(window, cx))
            .when(self.kg_alternatives.is_open(), |root| {
                root.child(self.render_kg_alternatives(cx))
            })
            // Editor area
            .child(
                div().flex_1().p_4().overflow_hidden().child(
                    div()
                        .size_full()
                        .flex()
                        .flex_row()
                        .overflow_hidden()
                        .when(!self.show_blocks, |row| {
                            row.child(self.render_document(editor_font, cx))
                        })
                        .when(self.show_blocks, |d| d.child(self.render_blocks(cx)))
                        .when_some(preview, |row, preview| {
                            row.child(
                                div()
                                    .debug_selector(|| "markdown-preview-pane".into())
                                    .flex_1()
                                    .h_full()
                                    .min_w_0()
                                    .border_l_1()
                                    .border_color(border)
                                    .p_3()
                                    .overflow_y_scrollbar()
                                    .child(preview),
                            )
                        }),
                ),
            )
            // Status bar
            .child(self.render_status_bar(window, cx))
            .when_some(self.inline_alternatives.as_ref(), |root, inline| {
                root.child(self.render_inline_alternatives(inline, cx))
            })
    }
}

/// Create a simple editor without autocomplete (for testing).
///
/// # Panics
/// Panics if the empty autocomplete index cannot be built.
pub fn create_simple_editor(
    title: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut Context<TerraphimEditor>,
) -> TerraphimEditor {
    use terraphim_automata::autocomplete::build_autocomplete_index;
    use terraphim_types::Thesaurus;

    // Create empty thesaurus and index
    let thesaurus = Thesaurus::new("Empty".to_string());
    let index = Arc::new(build_autocomplete_index(thesaurus, None).unwrap());

    TerraphimEditor::new(
        index,
        SlashCommandRegistry::with_defaults(),
        title,
        window,
        cx,
    )
}

#[cfg(test)]
mod tests {
    use super::{ReopenCompletions, RotateCompletionBackward, RotateCompletionForward};

    #[test]
    fn test_actions_defined() {
        // Just verify actions compile
        let _ = ReopenCompletions;
        let _ = RotateCompletionForward;
        let _ = RotateCompletionBackward;
    }
}

#[cfg(test)]
#[path = "editor_blocks_tests.rs"]
mod editor_blocks_tests;
