//! Slash Commands Story - Visual testing for slash command functionality
//!
//! Tests:
//! - '/' triggers command menu at line start
//! - Command filtering as you type
//! - Enter executes command
//! - Command output replaces input
//!
//! Run: cargo run -p terraphim-gpui-story --example slash_commands

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    div, prelude::*, px, App, Application, Context, Entity, IntoElement, Render, SharedString,
    Window, WindowOptions,
};
use gpui_component::{
    input::{Input, InputState},
    ActiveTheme, Root,
};
use terraphim_automata::autocomplete::{build_autocomplete_index, AutocompleteIndex};
use terraphim_gpui_editor::{CompletionConfig, SlashCommandRegistry, TerraphimCompletionProvider};
use terraphim_types::Thesaurus;

struct SlashCommandsStory {
    input_state: Entity<InputState>,
    commands_info: Vec<CommandInfo>,
}

struct CommandInfo {
    name: SharedString,
    description: SharedString,
    example: SharedString,
}

impl SlashCommandsStory {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Create minimal index (slash commands don't need terms)
        let index = create_minimal_index();
        let registry = SlashCommandRegistry::with_defaults();

        // Collect command info before moving registry
        let commands_info: Vec<CommandInfo> = registry
            .filter("")
            .into_iter()
            .map(|cmd| CommandInfo {
                name: SharedString::from(cmd.name().to_string()),
                description: SharedString::from(cmd.description().to_string()),
                example: SharedString::from(format!("/{}", cmd.name())),
            })
            .collect();

        // Create completion provider
        let provider = TerraphimCompletionProvider::new(
            index,
            registry,
            CompletionConfig::default(),
        );

        // Create input state with completion provider
        // Note: multi_line(true) is required for Tab/Shift+Tab to work with completions
        let input_state = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .multi_line(true)
                .placeholder("Type '/' at line start to see commands...");

            state.lsp.completion_provider = Some(Rc::new(provider));
            state
        });

        Self {
            input_state,
            commands_info,
        }
    }
}

impl Render for SlashCommandsStory {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                // Header
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Slash Commands Story"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Test /command execution and menu"),
                    ),
            )
            .child(
                // Main content
                div()
                    .flex_1()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    // Instructions
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .bg(theme.muted)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .mb_2()
                                    .child("Instructions:"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("- Type '/' at the start of a line to open command menu"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("- Continue typing to filter commands (e.g., '/he' shows help, heading)"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("- Press Tab/Enter to select and execute"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child("- Some commands accept arguments (e.g., '/heading 2')"),
                            ),
                    )
                    // Available commands
                    .child(
                        div()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .mb_2()
                                    .child("Available Commands:"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .children(self.commands_info.iter().map(|cmd| {
                                        div()
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .bg(theme.secondary)
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .font_family("monospace")
                                                            .text_sm()
                                                            .child(cmd.example.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(theme.muted_foreground)
                                                            .child(format!("- {}", cmd.description)),
                                                    ),
                                            )
                                    })),
                            ),
                    )
                    // Input field
                    .child(
                        div()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .mb_2()
                                    .child("Try it:"),
                            )
                            .child(Input::new(&self.input_state).cleanable(true)),
                    ),
            )
    }
}

/// Create minimal index for slash command testing
fn create_minimal_index() -> Arc<AutocompleteIndex> {
    let thesaurus = Thesaurus::new("minimal".to_string());
    Arc::new(build_autocomplete_index(thesaurus, None).expect("Failed to build minimal index"))
}

fn main() {
    env_logger::init();

    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);

        let options = WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                origin: gpui::Point::default(),
                size: gpui::Size {
                    width: px(700.0),
                    height: px(500.0),
                },
            })),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let story = cx.new(|cx| SlashCommandsStory::new(window, cx));
            cx.new(|cx| Root::new(story, window, cx))
        })
        .unwrap();
    });
}
