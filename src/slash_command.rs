//! Slash command system for the editor
//!
//! Provides a trait-based command system that allows registering and executing
//! slash commands (e.g., /help, /code, /heading).

use gpui::{App, Window};
use std::collections::HashMap;
use std::sync::Arc;

/// Result of executing a slash command
#[derive(Debug, Clone, Default)]
pub struct CommandOutput {
    /// Text to insert at cursor position
    pub insert_text: Option<String>,
    /// Whether to close the completion menu after execution
    pub close_menu: bool,
    /// Optional message to display to the user
    pub message: Option<String>,
}

impl CommandOutput {
    /// Create output that inserts text
    pub fn insert(text: impl Into<String>) -> Self {
        Self {
            insert_text: Some(text.into()),
            close_menu: true,
            message: None,
        }
    }

    /// Create output with a message
    pub fn message(msg: impl Into<String>) -> Self {
        Self {
            insert_text: None,
            close_menu: true,
            message: Some(msg.into()),
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
}

impl SlashCommandRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Register a slash command
    ///
    /// If a command with the same name exists, it will be replaced.
    pub fn register(&mut self, command: impl SlashCommand + 'static) {
        let name = command.name().to_string();
        self.commands.insert(name, Arc::new(command));
    }

    /// Register a boxed slash command
    pub fn register_boxed(&mut self, command: Arc<dyn SlashCommand>) {
        let name = command.name().to_string();
        self.commands.insert(name, command);
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
        use crate::commands::{CodeCommand, HeadingCommand, HelpCommand, ListCommand};

        let mut registry = Self::new();

        // We'll add HelpCommand last since it needs a reference to the registry
        registry.register(HeadingCommand);
        registry.register(CodeCommand);
        registry.register(ListCommand);

        // Clone registry for HelpCommand
        let registry_clone = registry.clone();
        registry.register(HelpCommand::new(registry_clone));

        registry
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
