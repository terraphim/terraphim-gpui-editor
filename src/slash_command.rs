//! Slash command system for the editor
//!
//! Provides a trait-based command system that allows registering and executing
//! slash commands (e.g., /help, /code, /heading).

use gpui::{App, Window};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use terraphim_automata::{
    autocomplete_search, build_autocomplete_index, fuzzy_autocomplete_search, AutocompleteIndex,
};
use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

/// Editor actions triggered by slash commands
#[derive(Debug, Clone)]
pub enum EditorAction {
    NormalizeIds,
    ToggleBlocks,
    OpenFile(String),
    SaveFile(Option<String>),
}

/// Result of executing a slash command
#[derive(Debug, Clone, Default)]
pub struct CommandOutput {
    /// Text to insert at cursor position
    pub insert_text: Option<String>,
    /// Whether to close the completion menu after execution
    pub close_menu: bool,
    /// Optional message to display to the user
    pub message: Option<String>,
    /// Optional editor action to run
    pub editor_action: Option<EditorAction>,
}

impl CommandOutput {
    /// Create output that inserts text
    pub fn insert(text: impl Into<String>) -> Self {
        Self {
            insert_text: Some(text.into()),
            close_menu: true,
            message: None,
            editor_action: None,
        }
    }

    /// Create output with a message
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            insert_text: None,
            close_menu: true,
            message: Some(msg.into()),
            editor_action: None,
        }
    }

    /// Create output that runs an editor action
    pub fn action(action: EditorAction) -> Self {
        Self {
            insert_text: Some(String::new()),
            close_menu: true,
            message: None,
            editor_action: Some(action),
        }
    }

    /// Create empty output (no-op)
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Trait for implementing slash commands
///
/// Commands are triggered by typing `/` followed by the command name.
/// They can optionally accept arguments and provide argument completions.
pub trait SlashCommand: Send + Sync {
    /// Command name without the leading `/`
    fn name(&self) -> &str;

    /// Short description shown in the completion menu
    fn description(&self) -> &str;

    /// Detailed help text (defaults to description)
    fn help(&self) -> &str {
        self.description()
    }

    /// Whether this command requires at least one argument
    fn requires_argument(&self) -> bool {
        false
    }

    /// Whether this command accepts arguments (even if not required)
    fn accepts_arguments(&self) -> bool {
        self.requires_argument()
    }

    /// Execute the command with the given arguments
    ///
    /// # Arguments
    /// * `args` - Arguments passed to the command (may be empty)
    /// * `window` - GPUI window reference
    /// * `cx` - GPUI application context
    fn execute(&self, args: &[String], window: &mut Window, cx: &mut App) -> CommandOutput;

    /// Provide completions for command arguments
    ///
    /// Override this to provide suggestions when the user is typing arguments.
    fn complete_argument(
        &self,
        _partial: &str,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Vec<String> {
        Vec::new()
    }
}

/// Registry of available slash commands
///
/// Commands are stored by name and can be looked up or filtered by prefix.
#[derive(Clone, Default)]
pub struct SlashCommandRegistry {
    commands: HashMap<String, Arc<dyn SlashCommand>>,
    command_index: Option<AutocompleteIndex>,
    command_index_terms: HashMap<String, Vec<String>>,
}

impl SlashCommandRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            command_index: None,
            command_index_terms: HashMap::new(),
        }
    }

    /// Register a slash command
    ///
    /// If a command with the same name exists, it will be replaced.
    pub fn register(&mut self, command: impl SlashCommand + 'static) {
        let name = command.name().to_string();
        self.commands.insert(name, Arc::new(command));
        self.rebuild_command_index();
    }

    /// Register a boxed slash command
    pub fn register_boxed(&mut self, command: Arc<dyn SlashCommand>) {
        let name = command.name().to_string();
        self.commands.insert(name, command);
        self.rebuild_command_index();
    }

    /// Get a command by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn SlashCommand>> {
        self.commands.get(name).cloned()
    }

    /// Get all command names
    pub fn names(&self) -> Vec<&str> {
        self.commands.keys().map(String::as_str).collect()
    }

    /// Filter commands by prefix
    ///
    /// Returns all commands whose names start with the given prefix.
    pub fn filter(&self, prefix: &str) -> Vec<Arc<dyn SlashCommand>> {
        let prefix_lower = prefix.to_lowercase();
        let indexed = self.commands_from_index(&prefix_lower);
        if !indexed.is_empty() {
            return indexed;
        }

        self.commands
            .iter()
            .filter(|(name, _)| name.to_lowercase().starts_with(&prefix_lower))
            .map(|(_, cmd)| cmd.clone())
            .collect()
    }

    /// Get the number of registered commands
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Create a registry with default built-in commands
    pub fn with_defaults() -> Self {
        use crate::commands::{
            BlocksCommand, CodeCommand, HeadingCommand, HelpCommand, IdsCommand, ListCommand,
            NormalizeCommand, OpenCommand, SaveCommand,
        };

        let mut registry = Self::new();

        // We'll add HelpCommand last since it needs a reference to the registry
        registry.register(HeadingCommand);
        registry.register(CodeCommand);
        registry.register(ListCommand);
        registry.register(IdsCommand);
        registry.register(NormalizeCommand);
        registry.register(BlocksCommand);
        registry.register(OpenCommand);
        registry.register(SaveCommand);

        registry.register_markdown_commands();

        // Clone registry for HelpCommand
        let registry_clone = registry.clone();
        registry.register(HelpCommand::new(registry_clone));

        registry
    }

    fn rebuild_command_index(&mut self) {
        if self.commands.is_empty() {
            self.command_index = None;
            self.command_index_terms.clear();
            return;
        }

        let mut thesaurus = Thesaurus::new("slash-commands".to_string());
        let mut term_to_commands: HashMap<String, Vec<String>> = HashMap::new();
        let mut seen_terms: HashSet<String> = HashSet::new();
        let mut next_id = 1u64;

        let mut names: Vec<&String> = self.commands.keys().collect();
        names.sort();

        for name in names {
            add_command_term(
                name,
                name,
                &mut term_to_commands,
                &mut thesaurus,
                &mut seen_terms,
                &mut next_id,
            );
        }

        match build_autocomplete_index(thesaurus, None) {
            Ok(index) => {
                self.command_index = Some(index);
                self.command_index_terms = term_to_commands;
            }
            Err(err) => {
                log::warn!("Failed to build command autocomplete index: {}", err);
                self.command_index = None;
                self.command_index_terms.clear();
            }
        }
    }

    fn commands_from_index(&self, query: &str) -> Vec<Arc<dyn SlashCommand>> {
        let Some(index) = &self.command_index else {
            return Vec::new();
        };

        let limit = Some(self.commands.len());
        let results = if query.len() < 3 {
            autocomplete_search(index, query, limit).unwrap_or_default()
        } else {
            fuzzy_autocomplete_search(index, query, 0.7, limit).unwrap_or_default()
        };

        let mut seen: HashSet<String> = HashSet::new();
        let mut commands = Vec::new();

        for result in results {
            let key = result.term.to_lowercase();
            if let Some(names) = self.command_index_terms.get(&key) {
                for name in names {
                    if seen.insert(name.clone()) {
                        if let Some(cmd) = self.commands.get(name) {
                            commands.push(cmd.clone());
                        }
                    }
                }
            }
        }

        commands
    }

    fn register_markdown_commands(&mut self) {
        let definitions = load_markdown_command_definitions();
        if definitions.is_empty() {
            return;
        }

        for definition in definitions {
            let primary = MarkdownCommand::from_definition(&definition, &definition.name);
            if !self.commands.contains_key(primary.name()) {
                self.register(primary);
            }

            for alias in &definition.aliases {
                let alias_cmd = MarkdownCommand::from_definition(&definition, alias);
                if !self.commands.contains_key(alias_cmd.name()) {
                    self.register(alias_cmd);
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct MarkdownCommandFrontmatter {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    usage: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    parameters: Vec<MarkdownCommandParameter>,
}

impl MarkdownCommandFrontmatter {
    fn accepts_args(&self) -> bool {
        if !self.parameters.is_empty() {
            return true;
        }
        self.usage
            .as_deref()
            .map(|usage| usage.contains('<') || usage.contains('['))
            .unwrap_or(false)
    }

    fn requires_args(&self) -> bool {
        self.parameters.iter().any(|param| param.required)
    }
}

#[derive(Debug, Deserialize)]
struct MarkdownCommandParameter {
    name: String,
    #[serde(default)]
    required: bool,
}

struct MarkdownCommand {
    name: String,
    description: String,
    usage: String,
    help_text: String,
    accepts_args: bool,
    requires_args: bool,
}

impl MarkdownCommand {
    fn from_definition(definition: &MarkdownCommandFrontmatter, name: &str) -> Self {
        let description = definition
            .description
            .clone()
            .unwrap_or_else(|| format!("Run {} command", name));
        let usage = definition
            .usage
            .as_deref()
            .map(|usage| rewrite_usage(usage, &definition.name, name))
            .unwrap_or_else(|| name.to_string());
        let help_text = format!("Usage: /{}\n\n{}", usage, description);

        Self {
            name: name.to_string(),
            description,
            usage,
            help_text,
            accepts_args: definition.accepts_args(),
            requires_args: definition.requires_args(),
        }
    }
}

impl SlashCommand for MarkdownCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn help(&self) -> &str {
        &self.help_text
    }

    fn requires_argument(&self) -> bool {
        self.requires_args
    }

    fn accepts_arguments(&self) -> bool {
        self.accepts_args
    }

    fn execute(&self, args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        let output = if args.is_empty() {
            format!("/{}", self.usage)
        } else {
            format!("/{} {}", self.name, args.join(" "))
        };

        CommandOutput::insert(output)
    }
}

fn load_markdown_command_definitions() -> Vec<MarkdownCommandFrontmatter> {
    let Some(commands_dir) = resolve_commands_dir() else {
        return Vec::new();
    };

    if !commands_dir.exists() {
        log::warn!(
            "Markdown command directory not found: {}",
            commands_dir.display()
        );
        return Vec::new();
    }

    let mut files = Vec::new();
    if let Err(err) = collect_markdown_files(&commands_dir, &mut files) {
        log::warn!(
            "Failed to enumerate markdown commands in {}: {}",
            commands_dir.display(),
            err
        );
        return Vec::new();
    }

    let mut definitions = Vec::new();
    for path in files {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                log::warn!("Failed to read markdown command {}: {}", path.display(), err);
                continue;
            }
        };

        let Some(frontmatter) = extract_frontmatter(&content) else {
            continue;
        };

        match serde_yaml::from_str::<MarkdownCommandFrontmatter>(&frontmatter) {
            Ok(definition) => definitions.push(definition),
            Err(err) => {
                log::warn!(
                    "Failed to parse markdown command {}: {}",
                    path.display(),
                    err
                );
            }
        }
    }

    definitions
}

fn resolve_commands_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("TERRAPHIM_COMMANDS_DIR") {
        if !dir.trim().is_empty() {
            return Some(PathBuf::from(dir));
        }
    }

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Some(
        crate_dir
            .join("../terraphim-ai/crates/terraphim_agent/commands"),
    )
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
}

fn extract_frontmatter(contents: &str) -> Option<String> {
    let mut lines = contents.lines();
    let first = lines.next()?.trim();
    if first != "---" {
        return None;
    }

    let mut yaml = String::new();
    for line in lines.by_ref() {
        if line.trim() == "---" {
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }

    if yaml.trim().is_empty() {
        None
    } else {
        Some(yaml)
    }
}

fn rewrite_usage(usage: &str, primary: &str, alias: &str) -> String {
    if let Some(rest) = usage.strip_prefix(primary) {
        format!("{}{}", alias, rest)
    } else {
        usage.to_string()
    }
}

fn add_command_term(
    term: &str,
    command_id: &str,
    term_to_commands: &mut HashMap<String, Vec<String>>,
    thesaurus: &mut Thesaurus,
    seen_terms: &mut HashSet<String>,
    next_id: &mut u64,
) {
    let normalized = term.trim().to_lowercase();
    if normalized.is_empty() {
        return;
    }

    term_to_commands
        .entry(normalized.clone())
        .or_default()
        .push(command_id.to_string());

    if seen_terms.insert(normalized.clone()) {
        let value = NormalizedTermValue::from(normalized.clone());
        let term_entry = NormalizedTerm::new(*next_id, value.clone());
        thesaurus.insert(value, term_entry);
        *next_id += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCommand {
        name: String,
        description: String,
    }

    impl TestCommand {
        fn new(name: &str, description: &str) -> Self {
            Self {
                name: name.to_string(),
                description: description.to_string(),
            }
        }
    }

    impl SlashCommand for TestCommand {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn execute(&self, _args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
            CommandOutput::insert(format!("Executed: {}", self.name))
        }
    }

    #[test]
    fn test_registry_register_and_get() {
        let mut registry = SlashCommandRegistry::new();
        registry.register(TestCommand::new("test", "A test command"));

        assert!(registry.get("test").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_filter() {
        let mut registry = SlashCommandRegistry::new();
        registry.register(TestCommand::new("help", "Show help"));
        registry.register(TestCommand::new("heading", "Insert heading"));
        registry.register(TestCommand::new("code", "Insert code"));

        let filtered = registry.filter("he");
        assert_eq!(filtered.len(), 2);

        let filtered = registry.filter("c");
        assert_eq!(filtered.len(), 1);

        let filtered = registry.filter("x");
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_registry_names() {
        let mut registry = SlashCommandRegistry::new();
        registry.register(TestCommand::new("a", "First"));
        registry.register(TestCommand::new("b", "Second"));

        let names = registry.names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn test_command_output_insert() {
        let output = CommandOutput::insert("Hello");
        assert_eq!(output.insert_text, Some("Hello".to_string()));
        assert!(output.close_menu);
    }

    #[test]
    fn test_command_output_message() {
        let output = CommandOutput::message("Info");
        assert!(output.insert_text.is_none());
        assert_eq!(output.message, Some("Info".to_string()));
    }
}
