//! Full Editor Story - Visual testing for complete TerraphimEditor
//!
//! Tests:
//! - Full editor component with all features
//! - Autocomplete + slash commands together
//! - Multi-line editing
//! - Keyboard shortcuts
//!
//! Run: cargo run -p terraphim-gpui-story --example editor

use std::sync::Arc;

use gpui::{
    div, prelude::*, px, App, Application, Context, Entity, IntoElement, Render, Window,
    WindowOptions,
};
use gpui_component::{ActiveTheme, Root};
use terraphim_automata::autocomplete::{build_autocomplete_index, AutocompleteIndex};
use terraphim_gpui_editor::{SlashCommandRegistry, TerraphimEditor};
use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

struct EditorStory {
    editor: Entity<TerraphimEditor>,
}

impl EditorStory {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let index = create_demo_index();
        let commands = SlashCommandRegistry::with_defaults();

        let editor = cx.new(|cx| {
            TerraphimEditor::new(index, commands, "Editor Story", window, cx)
        });

        Self { editor }
    }
}

impl Render for EditorStory {
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
                            .child("Full Editor Story"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Complete TerraphimEditor with all features"),
                    ),
            )
            .child(
                // Keyboard shortcuts reference
                div()
                    .px_4()
                    .py_2()
                    .bg(theme.muted)
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .text_xs()
                    .child(shortcut_item("Tab", "Accept/cycle completion"))
                    .child(shortcut_item("Shift+Tab", "Cycle backward"))
                    .child(shortcut_item("Ctrl+Space", "Reopen completions"))
                    .child(shortcut_item("Enter", "Execute slash command"))
                    .child(shortcut_item("Escape", "Close menu")),
            )
            .child(
                // Editor
                div()
                    .flex_1()
                    .p_4()
                    .child(self.editor.clone()),
            )
            .child(
                // Footer
                div()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Try: Type 'mach' for autocomplete, '/' for commands, '/heading 2' for H2"),
            )
    }
}

fn shortcut_item(key: &'static str, description: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .px_1()
                .rounded_sm()
                .bg(gpui::rgb(0x374151))
                .text_color(gpui::rgb(0xffffff))
                .child(key),
        )
        .child(description)
}

/// Create a demo autocomplete index with engineering/AI terms
fn create_demo_index() -> Arc<AutocompleteIndex> {
    let mut thesaurus = Thesaurus::new("demo_engineer".to_string());

    let terms = [
        // AI and ML terms
        ("machine learning", "machine_learning", 88),
        ("artificial neural network", "neural_networks", 87),
        ("deep learning", "deep_learning", 89),
        ("natural language processing", "nlp", 90),
        ("computer vision", "computer_vision", 91),
        // Operations terms
        ("system maintains key functions", "operation", 82),
        ("preventive maintenance", "maintenance", 84),
        ("corrective maintenance", "maintenance", 84),
        // Rust and programming
        ("rust programming", "rust", 100),
        ("cargo build", "cargo", 101),
        ("async await", "async", 102),
        ("trait bounds", "traits", 103),
        // Terraphim specific
        ("terraphim", "terraphim", 110),
        ("knowledge graph", "knowledge_graph", 111),
        ("semantic search", "semantic_search", 112),
        ("thesaurus", "thesaurus", 113),
        ("automata", "automata", 114),
        ("rolegraph", "rolegraph", 115),
    ];

    for (key, normalized, id) in terms {
        let normalized_term = NormalizedTerm::new(id, NormalizedTermValue::from(normalized))
            .with_url(format!(
                "https://terraphim.ai/terms/{}",
                normalized.replace('_', "-")
            ));
        thesaurus.insert(NormalizedTermValue::from(key), normalized_term);
    }

    Arc::new(build_autocomplete_index(thesaurus, None).expect("Failed to build demo index"))
}

fn main() {
    env_logger::init();

    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);

        let options = WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                origin: gpui::Point::default(),
                size: gpui::Size {
                    width: px(800.0),
                    height: px(600.0),
                },
            })),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let story = cx.new(|cx| EditorStory::new(window, cx));
            cx.new(|cx| Root::new(story, window, cx))
        })
        .unwrap();
    });
}
