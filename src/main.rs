//! Terraphim GPUI Editor
//!
//! A native desktop editor with intelligent autocomplete and slash commands.

use anyhow::Result;
use gpui::{
    px, size, App, AppContext, Application, Entity, KeyBinding, Pixels, SharedString, Size,
    TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_component::Root;
use gpui_component_assets::Assets;
use std::path::PathBuf;
use std::sync::Arc;
use terraphim_automata::autocomplete::{build_autocomplete_index, AutocompleteIndex};
use terraphim_gpui_editor::{RoleSource, SlashCommandRegistry, TerraphimEditor};
use terraphim_lsp_core::KgEngine;
use terraphim_types::Thesaurus;

#[cfg(any(target_os = "linux", all(unix, test)))]
mod omarchy;

#[cfg(target_os = "macos")]
mod tray;

#[cfg(all(unix, feature = "gdp"))]
mod headless;

const PLAIN_TEXT_ROLE: &str = "Plain text";

/// Application configuration
struct AppConfig {
    /// Standalone thesaurus JSON or Markdown KG source directory.
    role_path: Option<PathBuf>,
    roles: Vec<String>,
    /// Path to markdown file (optional)
    file_path: Option<PathBuf>,
    /// Window title
    title: String,
    /// Window size
    size: Size<Pixels>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            role_path: None,
            roles: Vec::new(),
            file_path: None,
            title: "Terraphim Editor".to_string(),
            size: size(px(900.), px(700.)),
        }
    }
}

struct LoadedRole {
    name: SharedString,
    index: Arc<AutocompleteIndex>,
    graph: KgEngine,
    source: Option<RoleSource>,
}

fn build_role(thesaurus: Thesaurus) -> Result<LoadedRole> {
    Ok(LoadedRole {
        name: SharedString::from(thesaurus.name().to_owned()),
        source: None,
        graph: KgEngine::new(&thesaurus)?,
        index: Arc::new(build_autocomplete_index(thesaurus, None)?),
    })
}

fn load_role(path: &std::path::Path) -> Result<LoadedRole> {
    let source = RoleSource::load(path)?;
    let mut role = build_role(source.thesaurus()?)?;
    role.source = Some(source);
    Ok(role)
}

fn load_roles(default: &AutocompleteIndex, entries: &[String]) -> Vec<LoadedRole> {
    let mut names = std::collections::HashSet::from([
        default.name().to_lowercase(),
        PLAIN_TEXT_ROLE.to_lowercase(),
        "no autocomplete".to_owned(),
    ]);
    let mut roles = Vec::new();
    for entry in entries {
        let Some((name, path)) = entry.split_once('=') else {
            log::warn!("Ignoring invalid role {entry:?}. Expected NAME=PATH");
            continue;
        };
        let name = name.trim();
        if name.is_empty() || names.contains(&name.to_lowercase()) {
            log::warn!("Ignoring empty or duplicate role name {name:?}");
            continue;
        }
        match load_role(std::path::Path::new(path)) {
            Ok(mut role) => {
                names.insert(name.to_lowercase());
                role.name = SharedString::from(name.to_owned());
                roles.push(role);
            }
            Err(error) => log::warn!("Could not load role {name}: {error:#}"),
        }
    }

    if !default.name().eq_ignore_ascii_case(PLAIN_TEXT_ROLE) || !default.is_empty() {
        let plain_name = if default.name().eq_ignore_ascii_case(PLAIN_TEXT_ROLE) {
            "No autocomplete"
        } else {
            PLAIN_TEXT_ROLE
        };
        let plain = Thesaurus::new(plain_name.to_owned());
        roles.push(build_role(plain).expect("empty thesaurus is valid"));
    }
    roles
}

/// Create a demo autocomplete index with sample terms
fn create_demo_thesaurus() -> Thesaurus {
    use terraphim_types::{NormalizedTerm, NormalizedTermValue};

    let mut thesaurus = Thesaurus::new("Demo".to_string());

    let terms = [
        // Technical terms
        ("machine learning", "machine learning", 100),
        ("ml", "machine learning", 100),
        ("artificial intelligence", "artificial intelligence", 95),
        ("neural network", "neural network", 90),
        ("deep learning", "deep learning", 88),
        (
            "natural language processing",
            "natural language processing",
            85,
        ),
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
        let normalized_term = NormalizedTerm::new(id, NormalizedTermValue::from(normalized))
            .with_url(format!(
                "https://terraphim.ai/terms/{}",
                key.replace(' ', "-")
            ));
        thesaurus.insert(NormalizedTermValue::from(key), normalized_term);
    }

    thesaurus
}

fn main() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    log::info!("Starting Terraphim Editor");

    if let Err(error) = try_kg_command() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }

    // Parse command line arguments
    let config = parse_args();

    #[cfg(all(unix, feature = "gdp"))]
    if let Some(socket) = std::env::var_os("GDP_SOCKET") {
        if let Err(error) = headless::serve(std::path::Path::new(&socket), config) {
            log::error!("Headless editor: {error:#}");
            std::process::exit(1);
        }
        return;
    }

    // Create GPUI application with assets
    let app = Application::new().with_assets(Assets);

    app.run(move |cx| {
        // Initialize gpui-component
        gpui_component::init(cx);
        #[cfg(any(target_os = "linux", all(unix, test)))]
        omarchy::install(cx);

        let window_options = editor_window_options(&config, cx);
        let editor = prepare_editor(config, cx);

        cx.spawn(async move |cx| {
            let mut opened_editor = None;
            let handle = cx.open_window(window_options, |window, cx| {
                let (root, view) = editor.open(window, cx);
                opened_editor = Some(view);
                root
            })?;
            #[cfg(target_os = "macos")]
            cx.update(|cx| {
                cx.activate(true);
                if let Err(error) =
                    tray::install(handle, &opened_editor.expect("opened editor window"), cx)
                {
                    log::warn!("Could not install the menu-bar icon: {error:#}");
                }
            })?;
            #[cfg(not(target_os = "macos"))]
            let _ = (handle, opened_editor);

            Ok::<_, anyhow::Error>(())
        })
        .detach_and_log_err(cx);
    });
}

fn try_kg_command() -> Result<()> {
    use std::io::{Read, Write};
    const LIMIT: u64 = 1024 * 1024;

    let mut arguments = std::env::args_os().skip(1);
    let Some(command) = arguments
        .next()
        .filter(|argument| argument == "--kg-request" || argument == "--kg-export")
    else {
        return Ok(());
    };
    let root = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Supply a Markdown role directory"))?;
    anyhow::ensure!(
        arguments.next().is_none(),
        "Expected only a Markdown role directory"
    );
    let mut output = std::io::stdout().lock();
    if command == "--kg-export" {
        anyhow::ensure!(root.is_dir(), "Supply a Markdown role directory");
        serde_json::to_writer_pretty(&mut output, &RoleSource::load(&root)?.thesaurus()?)?;
    } else {
        let mut payload = Vec::new();
        std::io::stdin()
            .lock()
            .take(LIMIT + 1)
            .read_to_end(&mut payload)?;
        anyhow::ensure!(
            payload.len() as u64 <= LIMIT,
            "Knowledge graph request exceeds 1 MiB"
        );
        serde_json::to_writer(
            &mut output,
            &terraphim_gpui_editor::markdown_kg_request(&root, &payload),
        )?;
    }
    writeln!(&mut output)?;
    output.flush()?;
    std::process::exit(0);
}

struct PreparedEditor {
    index: Arc<AutocompleteIndex>,
    roles: Vec<(SharedString, Arc<AutocompleteIndex>)>,
    graphs: std::collections::HashMap<SharedString, KgEngine>,
    sources: std::collections::HashMap<SharedString, RoleSource>,
    commands: SlashCommandRegistry,
    file_path: Option<PathBuf>,
}

fn prepare_editor(config: AppConfig, cx: &mut App) -> PreparedEditor {
    register_keybindings(cx);
    let default = config
        .role_path
        .as_deref()
        .and_then(|path| match load_role(path) {
            Ok(index) => Some(index),
            Err(error) => {
                log::error!("Failed to load role: {error:#}");
                None
            }
        })
        .unwrap_or_else(|| build_role(create_demo_thesaurus()).expect("valid demo thesaurus"));
    let index = default.index;
    let mut sources = std::collections::HashMap::new();
    if let Some(source) = default.source {
        sources.insert(default.name.clone(), source);
    }
    let mut graphs = std::collections::HashMap::from([(default.name, default.graph)]);
    let roles = load_roles(&index, &config.roles)
        .into_iter()
        .map(|role| {
            if let Some(source) = role.source {
                sources.insert(role.name.clone(), source);
            }
            graphs.insert(role.name.clone(), role.graph);
            (role.name, role.index)
        })
        .collect();
    log::info!("Loaded role {} with {} terms", index.name(), index.len());
    let commands = SlashCommandRegistry::with_defaults();
    log::info!("Registered {} slash commands", commands.len());
    PreparedEditor {
        index,
        roles,
        graphs,
        sources,
        commands,
        file_path: config.file_path,
    }
}

fn editor_window_options(config: &AppConfig, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(config.size, cx)),
        titlebar: Some(TitlebarOptions {
            title: Some(config.title.clone().into()),
            // Keep the editor toolbar below the native macOS window controls.
            appears_transparent: false,
            ..Default::default()
        }),
        ..Default::default()
    }
}

impl PreparedEditor {
    fn open(self, window: &mut Window, cx: &mut App) -> (Entity<Root>, Entity<TerraphimEditor>) {
        let editor = cx.new(|cx| {
            TerraphimEditor::new(self.index, self.commands, "Terraphim Editor", window, cx)
                .with_roles(self.roles)
                .with_role_graphs(self.graphs)
                .with_role_sources(self.sources)
        });
        if let Some(path) = self.file_path {
            editor.update(cx, |editor, cx| {
                if let Err(err) = editor.open_file(path, window, cx) {
                    log::error!("Failed to open file: {err}");
                }
            });
        }
        let focus = editor.downgrade();
        window.defer(cx, move |window, cx| {
            _ = focus.update(cx, |editor, cx| editor.focus_document(window, cx));
        });
        (cx.new(|cx| Root::new(editor.clone(), window, cx)), editor)
    }
}

/// Register global keybindings
fn register_keybindings(cx: &mut App) {
    use terraphim_gpui_editor::editor::{
        Cancel, CommitBlockDraft, EditAlternatives, NextSynonym, PreviousSynonym,
        ReopenCompletions, RotateCompletionBackward, RotateCompletionForward, ToggleBlocksView,
    };

    cx.bind_keys([
        KeyBinding::new("ctrl-shift-b", ToggleBlocksView, None),
        KeyBinding::new(
            "ctrl-enter",
            CommitBlockDraft,
            Some("TerraphimBlockDraft > Input"),
        ),
        KeyBinding::new(
            "ctrl-shift-a",
            EditAlternatives,
            Some("TerraphimDocument > Input"),
        ),
        KeyBinding::new("alt-down", NextSynonym, Some("TerraphimDocument")),
        KeyBinding::new("alt-up", PreviousSynonym, Some("TerraphimDocument")),
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
            "--thesaurus" | "-t" | "--kg-dir" => {
                if let Some(path) = args.get(i + 1).filter(|arg| !arg.starts_with('-')) {
                    config.role_path = Some(PathBuf::from(path));
                    i += 1;
                } else {
                    log::warn!("Missing role source path after {}", args[i]);
                }
            }
            "--role" => {
                if let Some(role) = args.get(i + 1).filter(|arg| !arg.starts_with("--")) {
                    config.roles.push(role.clone());
                    i += 1;
                } else {
                    log::warn!("Missing NAME=PATH after --role");
                }
            }
            "--title" => {
                if i + 1 < args.len() {
                    config.title.clone_from(&args[i + 1]);
                    i += 1;
                }
            }
            "--file" | "-f" => {
                if i + 1 < args.len() {
                    config.file_path = Some(PathBuf::from(&args[i + 1]));
                    i += 1;
                }
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {
                // First positional arg is thesaurus path
                if config.role_path.is_none() && !args[i].starts_with('-') {
                    config.role_path = Some(PathBuf::from(&args[i]));
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
        r"Terraphim GPUI Editor

A native desktop editor with intelligent autocomplete and slash commands.

USAGE:
    terraphim-gpui-editor [OPTIONS] [THESAURUS_PATH]

OPTIONS:
    -t, --thesaurus <PATH>    Path to standalone thesaurus JSON
        --kg-dir <PATH>       Load a role from Markdown KG records
        --kg-export <PATH>    Print a Markdown role as thesaurus JSON
        --role <NAME=PATH>    Add a JSON or Markdown directory role (repeatable)
    -f, --file <PATH>         Path to markdown file to open
        --title <TITLE>       Window title
    -h, --help                Print help

EXAMPLES:
    terraphim-gpui-editor                        # Use demo thesaurus
    terraphim-gpui-editor my_thesaurus.json      # Load custom thesaurus
    terraphim-gpui-editor -t /path/to/thesaurus.json
    terraphim-gpui-editor --kg-dir .terraphim/kg -f notes.md

KEYBINDINGS:
    /           Start slash command
    Ctrl+Space  Reopen completions
    Tab         Next completion
    Shift+Tab   Previous completion
    Escape      Close completions
    Enter       Accept completion or new line
"
    );
}

#[cfg(test)]
mod tests {
    use super::{build_role, create_demo_thesaurus, load_roles, Thesaurus, PLAIN_TEXT_ROLE};
    use terraphim_automata::autocomplete::{autocomplete_search, AutocompleteResult};
    use terraphim_types::{NormalizedTerm, NormalizedTermValue};

    #[test]
    fn role_files_provide_distinct_autocomplete_and_skip_invalid_entries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("engineering.json");
        let mut thesaurus = Thesaurus::new("File name".to_owned());
        let term = NormalizedTermValue::from("mechanical drawing");
        thesaurus.insert(term.clone(), NormalizedTerm::new(1, term));
        std::fs::write(&path, serde_json::to_vec(&thesaurus).unwrap()).unwrap();
        let entries = [
            "Engineering=missing.json".to_owned(),
            format!("Engineering={}", path.display()),
            format!("engineering={}", path.display()),
            format!("Plain text={}", path.display()),
            "invalid".to_owned(),
        ];

        let default = build_role(create_demo_thesaurus()).unwrap().index;
        let roles = load_roles(&default, &entries);
        assert_eq!(roles.len(), 2);
        assert_eq!(roles[0].name.as_ref(), "Engineering");
        let results = autocomplete_search(&roles[0].index, "mechanical", None).unwrap();
        assert_eq!(results[0].term, "mechanical drawing");
        assert_eq!(
            autocomplete_search(&default, "mechanical", None).unwrap(),
            [] as [AutocompleteResult; 0]
        );
        assert_eq!(roles[1].name.as_ref(), PLAIN_TEXT_ROLE);
        assert!(roles[1].index.is_empty());
    }
    #[test]
    fn named_role_accepts_a_markdown_directory() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("mechanical drawing.md"),
            "# Mechanical Drawing\n\nsynonyms:: engineering sketch\n",
        )
        .unwrap();
        let default = build_role(create_demo_thesaurus()).unwrap().index;
        let roles = load_roles(
            &default,
            &[format!("Engineering={}", directory.path().display())],
        );
        assert_eq!(roles[0].name.as_ref(), "Engineering");
        let results = autocomplete_search(&roles[0].index, "engineering sk", None).unwrap();
        assert_eq!(results[0].term, "engineering sketch");
        assert!(roles[0].source.is_some());
    }
}
