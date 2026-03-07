//! Autocomplete Story - Visual testing for autocomplete functionality
//!
//! Tests:
//! - Tab to accept completion and cycle forward
//! - Shift+Tab to cycle backward
//! - Term completion from thesaurus
//! - Completion menu display (3-5 items)
//!
//! Run: cargo run -p terraphim-gpui-story --example autocomplete

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
use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

struct AutocompleteStory {
    input_state: Entity<InputState>,
    instructions: Vec<SharedString>,
}

impl AutocompleteStory {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Create test thesaurus with engineering terms
        let index = create_demo_index();

        // Create completion provider
        let provider = TerraphimCompletionProvider::new(
            index,
            SlashCommandRegistry::with_defaults(),
            CompletionConfig::default(),
        );

        // Create input state with completion provider
        // Note: multi_line(true) is required for Tab/Shift+Tab to work with completions
        let input_state = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .multi_line(true)
                .placeholder("Type to see autocomplete (try: 'mach', 'art', 'ter')...");

            // Set completion provider
            state.lsp.completion_provider = Some(Rc::new(provider));
            state
        });

        Self {
            input_state,
            instructions: vec![
                "Type 'mach' to see 'machine learning' suggestion".into(),
                "Type 'art' to see 'artificial neural network' suggestion".into(),
                "Type 'ter' to see 'terraphim' and related terms".into(),
                "Press Tab to accept completion and cycle forward".into(),
                "Press Shift+Tab to cycle backward through completions".into(),
                "Press Ctrl+Space to reopen completions".into(),
                "Type '/' at line start to see slash commands".into(),
            ],
        }
    }
}

impl Render for AutocompleteStory {
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
                            .child("Autocomplete Story"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Test Tab/Shift+Tab completion rotation"),
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
                            .children(self.instructions.iter().map(|instruction| {
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("- {}", instruction))
                            })),
                    )
                    // Input field
                    .child(
                        div()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .mb_2()
                                    .child("Editor Input:"),
                            )
                            .child(Input::new(&self.input_state).cleanable(true)),
                    ),
            )
            .child(
                // Footer with available terms
                div()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Available terms: machine learning, artificial neural network, deep learning, natural language processing, terraphim, knowledge graph, semantic search, rust programming, cargo build, async await"),
                    ),
            )
    }
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
        let normalized_term = NormalizedTerm {
            id,
            value: NormalizedTermValue::from(normalized),
            url: Some(format!(
                "https://terraphim.ai/terms/{}",
                normalized.replace('_', "-")
            )),
        };
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
                    width: px(700.0),
                    height: px(500.0),
                },
            })),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let story = cx.new(|cx| AutocompleteStory::new(window, cx));
            cx.new(|cx| Root::new(story, window, cx))
        })
        .unwrap();
    });
}
