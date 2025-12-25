//! Terraphim GPUI Editor
//!
//! A native desktop editor with intelligent autocomplete and slash commands.

use anyhow::Result;
use gpui::*;
use gpui_component::Root;
use gpui_component_assets::Assets;
use std::path::PathBuf;
use std::sync::Arc;
use terraphim_automata::autocomplete::{build_autocomplete_index, AutocompleteIndex};
use terraphim_gpui_editor::{SlashCommandRegistry, TerraphimEditor};
use terraphim_types::Thesaurus;

/// Application configuration
struct AppConfig {
    /// Path to thesaurus JSON file (optional)
    thesaurus_path: Option<PathBuf>,
    /// Window title
    title: String,
    /// Window size
    size: Size<Pixels>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            thesaurus_path: None,
            title: "Terraphim Editor".to_string(),
            size: size(px(900.), px(700.)),
        }
    }
}

/// Load autocomplete index from thesaurus file or create demo index
fn load_autocomplete_index(config: &AppConfig) -> Result<Arc<AutocompleteIndex>> {
    if let Some(path) = &config.thesaurus_path {
        log::info!("Loading thesaurus from: {:?}", path);
        let content = std::fs::read_to_string(path)?;
        let thesaurus: Thesaurus = serde_json::from_str(&content)?;
        let index = build_autocomplete_index(thesaurus, None)?;
        return Ok(Arc::new(index));
    }

    // Create demo thesaurus for testing
    log::info!("Creating demo thesaurus");
    let index = create_demo_index();
    Ok(Arc::new(index))
}

/// Create a demo autocomplete index with sample terms
fn create_demo_index() -> AutocompleteIndex {
    use terraphim_types::{NormalizedTerm, NormalizedTermValue};

    let mut thesaurus = Thesaurus::new("Demo".to_string());

    let terms = [
        // Technical terms
        ("machine learning", "machine learning", 100),
        ("artificial intelligence", "artificial intelligence", 95),
        ("neural network", "neural network", 90),
        ("deep learning", "deep learning", 88),
        ("natural language processing", "natural language processing", 85),
        ("computer vision", "computer vision", 82),
        ("reinforcement learning", "reinforcement learning", 80),
        // Rust-related
        ("rust", "rust programming", 75),
        ("cargo", "cargo build", 72),
        ("tokio", "tokio async", 70),
        ("async", "async await", 68),
        ("trait", "trait bounds", 65),
        ("lifetime", "lifetime annotation", 62),
        // Terraphim-specific
        ("terraphim", "terraphim ai", 98),
        ("knowledge graph", "knowledge graph", 92),
        ("thesaurus", "thesaurus", 88),
        ("automata", "automata", 85),
        ("rolegraph", "rolegraph", 82),
        // General
        ("hello", "hello world", 50),
        ("example", "example", 45),
        ("test", "test", 40),
    ];

    for (key, normalized, id) in terms {
        let normalized_term = NormalizedTerm {
            id,
            value: NormalizedTermValue::from(normalized),
            url: Some(format!("https://terraphim.ai/terms/{}", key.replace(' ', "-"))),
        };
        thesaurus.insert(NormalizedTermValue::from(key), normalized_term);
    }

    build_autocomplete_index(thesaurus, None).expect("Failed to build demo index")
}

fn main() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    log::info!("Starting Terraphim Editor");

    // Parse command line arguments
    let config = parse_args();

    // Create GPUI application with assets
    let app = Application::new().with_assets(Assets);

    app.run(move |cx| {
        // Initialize gpui-component
        gpui_component::init(cx);

        // Register keyboard shortcuts
        register_keybindings(cx);

        // Load autocomplete index
        let index = match load_autocomplete_index(&config) {
            Ok(idx) => {
                log::info!("Loaded autocomplete index with {} terms", idx.len());
                idx
            }
            Err(e) => {
                log::error!("Failed to load thesaurus: {}", e);
                log::info!("Using demo index instead");
                Arc::new(create_demo_index())
            }
        };

        // Create slash command registry
        let commands = SlashCommandRegistry::with_defaults();
        log::info!("Registered {} slash commands", commands.len());

        // Window options
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(config.size, cx)),
            titlebar: Some(TitlebarOptions {
                title: Some(config.title.clone().into()),
                appears_transparent: true,
                ..Default::default()
            }),
            ..Default::default()
        };

        // Open main window
        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let editor = cx.new(|cx| {
                    TerraphimEditor::new(index, commands, "Terraphim Editor", window, cx)
                });

                // Wrap in Root for proper theming
                cx.new(|cx| Root::new(editor, window, cx))
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach_and_log_err(cx);
    });
}

/// Register global keybindings
fn register_keybindings(cx: &mut App) {
    use terraphim_gpui_editor::editor::*;

    cx.bind_keys([
        // Reopen completions
        KeyBinding::new("ctrl-space", ReopenCompletions, None),
        // Completion navigation (these may be overridden by Input)
        KeyBinding::new("tab", RotateCompletionForward, None),
        KeyBinding::new("shift-tab", RotateCompletionBackward, None),
        // Cancel/escape
        KeyBinding::new("escape", Cancel, None),
    ]);
}

/// Parse command line arguments
fn parse_args() -> AppConfig {
    let args: Vec<String> = std::env::args().collect();
    let mut config = AppConfig::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--thesaurus" | "-t" => {
                if i + 1 < args.len() {
                    config.thesaurus_path = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            "--title" => {
                if i + 1 < args.len() {
                    config.title = args[i + 1].clone();
                    i += 1;
                }
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                // First positional arg is thesaurus path
                if config.thesaurus_path.is_none() && !args[i].starts_with('-') {
                    config.thesaurus_path = Some(PathBuf::from(&args[i]));
                }
            }
        }
        i += 1;
    }

    config
}

/// Print help message
fn print_help() {
    println!(
        r#"Terraphim GPUI Editor

A native desktop editor with intelligent autocomplete and slash commands.

USAGE:
    terraphim-gpui-editor [OPTIONS] [THESAURUS_PATH]

OPTIONS:
    -t, --thesaurus <PATH>    Path to thesaurus JSON file
        --title <TITLE>       Window title
    -h, --help                Print help

EXAMPLES:
    terraphim-gpui-editor                        # Use demo thesaurus
    terraphim-gpui-editor my_thesaurus.json      # Load custom thesaurus
    terraphim-gpui-editor -t /path/to/thesaurus.json

KEYBINDINGS:
    /           Start slash command
    Ctrl+Space  Reopen completions
    Tab         Next completion
    Shift+Tab   Previous completion
    Escape      Close completions
    Enter       Accept completion or new line
"#
    );
}
