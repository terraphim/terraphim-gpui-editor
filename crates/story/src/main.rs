//! Story Gallery - Visual testing for terraphim-gpui-editor
//!
//! Run with: cargo run -p terraphim-gpui-story
//!
//! Or run individual examples:
//! - cargo run -p terraphim-gpui-story --example autocomplete
//! - cargo run -p terraphim-gpui-story --example slash_commands
//! - cargo run -p terraphim-gpui-story --example editor

use gpui::{
    div, prelude::*, px, App, Application, Context, IntoElement, Render,
    SharedString, Window, WindowOptions,
};
use gpui_component::ActiveTheme;

struct StoryGallery {
    stories: Vec<StoryItem>,
}

struct StoryItem {
    name: SharedString,
    description: SharedString,
    command: SharedString,
}

impl StoryGallery {
    fn new() -> Self {
        Self {
            stories: vec![
                StoryItem {
                    name: "Autocomplete".into(),
                    description: "Test Tab/Shift+Tab completion rotation".into(),
                    command: "cargo run -p terraphim-gpui-story --example autocomplete".into(),
                },
                StoryItem {
                    name: "Slash Commands".into(),
                    description: "Test /command execution and menu".into(),
                    command: "cargo run -p terraphim-gpui-story --example slash_commands".into(),
                },
                StoryItem {
                    name: "Full Editor".into(),
                    description: "Complete editor with all features".into(),
                    command: "cargo run -p terraphim-gpui-story --example editor".into(),
                },
            ],
        }
    }
}

impl Render for StoryGallery {
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
                            .child("Terraphim GPUI Story Gallery"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Visual testing for editor components"),
                    ),
            )
            .child(
                // Story list
                div()
                    .flex_1()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .children(self.stories.iter().map(|story| {
                        div()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.secondary)
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(story.name.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .mt_1()
                                    .child(story.description.clone()),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .p_2()
                                    .rounded_md()
                                    .bg(theme.muted)
                                    .font_family("monospace")
                                    .text_sm()
                                    .child(story.command.clone()),
                            )
                    })),
            )
            .child(
                // Footer
                div()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Run examples from terminal to test specific features"),
            )
    }
}

fn main() {
    env_logger::init();

    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);

        let options = WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                origin: gpui::Point::default(),
                size: gpui::Size {
                    width: px(600.0),
                    height: px(500.0),
                },
            })),
            ..Default::default()
        };

        cx.open_window(options, |_window, cx| {
            cx.new(|_cx| StoryGallery::new())
        })
        .unwrap();
    });
}
