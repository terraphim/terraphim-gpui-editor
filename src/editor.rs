//! Terraphim Editor component
//!
//! Main editor view that combines the Input component with completion provider
//! and slash command support.

use crate::completion::{CompletionConfig, TerraphimCompletionProvider};
use crate::slash_command::{CommandOutput, EditorAction, SlashCommandRegistry};
use anyhow::{anyhow, Result as AnyhowResult};
use gpui::*;
use gpui::prelude::FluentBuilder;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement;
use gpui_component::ActiveTheme;
use gpui_component::Sizable;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use terraphim_automata::autocomplete::AutocompleteIndex;
use terraphim_markdown_parser::{BlockKind, NormalizedMarkdown};

// Actions for the editor
actions!(
    terraphim_editor,
    [
        ReopenCompletions,
        RotateCompletionForward,
        RotateCompletionBackward,
        ExecuteCommand,
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
    /// Title for the editor window
    title: SharedString,
    /// Whether to show block sidebar
    show_blocks: bool,
    /// Cached block list for sidebar display
    blocks: Vec<BlockPreview>,
    /// Last normalization/parsing error (if any)
    block_error: Option<SharedString>,
    /// Current file path (if loaded from disk)
    file_path: Option<PathBuf>,
    /// Subscriptions kept alive with the editor
    _subscriptions: Vec<Subscription>,
}

#[derive(Debug, Clone)]
struct BlockPreview {
    id: SharedString,
    kind: BlockKind,
    title: SharedString,
}

impl TerraphimEditor {
    /// Create a new Terraphim Editor
    ///
    /// # Arguments
    /// * `index` - Pre-built autocomplete index from terraphim_automata
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

        // Subscribe to input events
        let _subscriptions = vec![cx.subscribe_in(&input_state, window, {
            let input_state = input_state.clone();
            move |this, _, ev: &InputEvent, window, cx| {
                this.handle_input_event(&input_state, ev, window, cx);
            }
        })];

        Self {
            input_state,
            commands,
            title: title.into(),
            show_blocks: false,
            blocks: Vec::new(),
            block_error: None,
            file_path: None,
            _subscriptions,
        }
    }

    fn apply_normalized_markdown(
        &mut self,
        normalized: NormalizedMarkdown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let docs = terraphim_markdown_parser::blocks_to_documents("editor", &normalized);
        self.blocks = normalized
            .blocks
            .iter()
            .zip(docs.iter())
            .map(|(block, doc)| BlockPreview {
                id: block.id.to_string().into(),
                kind: block.kind,
                title: doc.title.clone().into(),
            })
            .collect();

        self.block_error = None;
        self.input_state.update(cx, |state, cx| {
            state.set_value(normalized.markdown.clone(), window, cx);
        });
        cx.notify();
    }

    fn normalize_and_refresh_blocks(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyhowResult<()> {
        let current = self.input_state.read(cx).value().to_string();
        let normalized = match terraphim_markdown_parser::normalize_markdown(&current) {
            Ok(normalized) => normalized,
            Err(err) => {
                self.block_error = Some(err.to_string().into());
                cx.notify();
                return Err(err.into());
            }
        };

        self.apply_normalized_markdown(normalized, window, cx);
        Ok(())
    }

    fn toggle_blocks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_blocks = !self.show_blocks;
        if self.show_blocks {
            let _ = self.normalize_and_refresh_blocks(window, cx);
        } else {
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
                // Text changed - completions are handled by the provider
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
            log::info!("Executing slash command: /{} {:?}", command_name, args);
            Some(cmd.execute(&args, window, cx))
        } else {
            log::warn!("Unknown slash command: /{}", command_name);
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
                        log::error!("Failed to open file: {}", err);
                    }
                }
                EditorAction::SaveFile(path) => {
                    let path = path.map(PathBuf::from);
                    match self.save_file(path, window, cx) {
                        Ok(saved) => {
                            log::info!("Saved markdown to {}", saved.display());
                        }
                        Err(err) => {
                            log::error!("Failed to save markdown: {}", err);
                        }
                    }
                }
            }
        }

        // If there's text to insert, replace the command line
        if let Some(insert_text) = output.insert_text {
            let current_text = self.input_state.read(cx).value().to_string();
            let new_text = current_text.replace(command_line, &insert_text);

            self.input_state.update(cx, |state, cx| {
                state.set_value(new_text, window, cx);
            });
        }

        // Log any message
        if let Some(msg) = output.message {
            log::info!("Command output: {}", msg);
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
            self.block_error = Some(err.to_string().into());
            self.input_state.update(cx, |state, cx| {
                state.set_value(raw, window, cx);
            });
            cx.notify();
        }
    }

    /// Load markdown, normalize IDs, and refresh block index.
    pub fn load_markdown(
        &mut self,
        markdown: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), terraphim_markdown_parser::MarkdownParserError> {
        let normalized = terraphim_markdown_parser::normalize_markdown(markdown)?;
        self.apply_normalized_markdown(normalized, window, cx);
        Ok(())
    }

    /// Open a markdown file from disk.
    pub fn open_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyhowResult<()> {
        let content = std::fs::read_to_string(&path)
            .map_err(|err| anyhow!("Failed to read {}: {}", path.display(), err))?;
        self.file_path = Some(path.clone());
        self.title = format!("Terraphim Editor — {}", path.display()).into();
        self.load_markdown(&content, window, cx)?;
        Ok(())
    }

    /// Save the current markdown to disk (normalized).
    pub fn save_file(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyhowResult<PathBuf> {
        let current = self.input_state.read(cx).value().to_string();
        let normalized = terraphim_markdown_parser::normalize_markdown(&current)?;
        let path = path
            .or_else(|| self.file_path.clone())
            .ok_or_else(|| anyhow!("No file path set. Use /save <path>"))?;
        std::fs::write(&path, &normalized.markdown)
            .map_err(|err| anyhow!("Failed to write {}: {}", path.display(), err))?;
        self.file_path = Some(path.clone());
        self.apply_normalized_markdown(normalized, window, cx);
        Ok(path)
    }

    /// Return normalized markdown for saving.
    pub fn normalized_text(
        &self,
        cx: &App,
    ) -> Result<String, terraphim_markdown_parser::MarkdownParserError> {
        terraphim_markdown_parser::ensure_terraphim_block_ids(&self.text(cx))
    }

    /// Render the toolbar
    fn render_toolbar(&self, _window: &mut Window, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

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
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Type / for commands | Ctrl+Space for completions"),
                    )
                    .child(
                        Button::new("terraphim-ids-btn")
                            .label("IDs")
                            .small()
                            .ghost()
                            .tooltip("Insert missing Terraphim block IDs")
                            .on_click(cx.listener(|this, _ev, window, cx| {
                                let _ = this.normalize_and_refresh_blocks(window, cx);
                            })),
                    )
                    .child(
                        Button::new("terraphim-blocks-btn")
                            .label(if self.show_blocks { "Blocks ✓" } else { "Blocks" })
                            .small()
                            .ghost()
                            .tooltip("Toggle block sidebar")
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.toggle_blocks(window, cx);
                            })),
                    ),
            )
    }

    fn render_blocks_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .w(px(320.))
            .h_full()
            .flex()
            .flex_col()
            .gap_2()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .p_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(format!("Blocks ({})", self.blocks.len())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Run IDs after edits"),
                    ),
            )
            .when_some(self.block_error.clone(), |d, err| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(theme.danger_foreground)
                        .child(err),
                )
            })
            .child(
                div()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .children(self.blocks.iter().map(|block| {
                        let kind = match block.kind {
                            BlockKind::Paragraph => "¶",
                            BlockKind::ListItem => "•",
                        };

                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p_2()
                            .rounded_md()
                            .bg(theme.background)
                            .border_1()
                            .border_color(theme.border)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{kind} {}", block.id)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.foreground)
                                    .child(block.title.clone()),
                            )
                    })),
            )
    }

    /// Render the status bar
    fn render_status_bar(&self, _window: &mut Window, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let text = self.input_state.read(cx).value();
        let char_count = text.chars().count();
        let word_count = text.split_whitespace().count();

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
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} chars", char_count)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} words", word_count)),
            )
    }
}

impl Render for TerraphimEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            // Toolbar
            .child(self.render_toolbar(window, cx))
            // Editor area
            .child(
                div()
                    .flex_1()
                    .p_4()
                    .overflow_hidden()
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .flex_row()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .child(
                                        Input::new(&self.input_state)
                                            .h_full()
                                            .text_base()
                                            .font_family("Berkeley Mono, SF Mono, Menlo, monospace"),
                                    ),
                            )
                            .when(self.show_blocks, |d| d.child(self.render_blocks_sidebar(cx))),
                    ),
            )
            // Status bar
            .child(self.render_status_bar(window, cx))
    }
}

/// Create a simple editor without autocomplete (for testing)
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
    use super::*;

    #[test]
    fn test_actions_defined() {
        // Just verify actions compile
        let _ = ReopenCompletions;
        let _ = RotateCompletionForward;
        let _ = RotateCompletionBackward;
    }
}
