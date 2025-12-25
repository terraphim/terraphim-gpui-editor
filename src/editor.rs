//! Terraphim Editor component
//!
//! Main editor view that combines the Input component with completion provider
//! and slash command support.

use crate::completion::{CompletionConfig, TerraphimCompletionProvider};
use crate::slash_command::SlashCommandRegistry;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::ActiveTheme;
use std::rc::Rc;
use std::sync::Arc;
use terraphim_automata::autocomplete::AutocompleteIndex;

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
    /// Title for the editor window
    title: SharedString,
    /// Subscriptions kept alive with the editor
    _subscriptions: Vec<Subscription>,
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
        // Create input state with multi-line support and completion provider
        let input_state = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .multi_line(true)
                .placeholder("Start typing... Use / for commands");

            // Set completion provider via the lsp field
            state.lsp.completion_provider = Some(Rc::new(TerraphimCompletionProvider::new(
                index,
                commands,
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
            title: title.into(),
            _subscriptions,
        }
    }

    /// Handle input events
    fn handle_input_event(
        &mut self,
        _input_state: &Entity<InputState>,
        event: &InputEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                // Text changed - completions are handled by the provider
                cx.notify();
            }
            InputEvent::PressEnter { secondary } => {
                // Enter pressed - could execute command or insert newline
                log::debug!("PressEnter event received, secondary: {}", secondary);
            }
            InputEvent::Focus => {
                log::debug!("Input focused");
            }
            InputEvent::Blur => {
                log::debug!("Input blurred");
            }
        }
    }

    /// Get the current text content
    pub fn text(&self, cx: &App) -> String {
        self.input_state.read(cx).value().to_string()
    }

    /// Set the text content
    pub fn set_text(&self, text: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        let text: SharedString = text.into();
        self.input_state.update(cx, |state, cx| {
            state.set_value(text, window, cx);
        });
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
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Type / for commands | Ctrl+Space for completions"),
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
                        Input::new(&self.input_state)
                            .h_full()
                            .text_base()
                            .font_family("Berkeley Mono, SF Mono, Menlo, monospace"),
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
