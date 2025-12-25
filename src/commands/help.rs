//! /help command - Show available commands

use crate::slash_command::{CommandOutput, SlashCommand, SlashCommandRegistry};
use gpui::{App, Window};

/// Shows a list of all available slash commands
pub struct HelpCommand {
    registry: SlashCommandRegistry,
}

impl HelpCommand {
    /// Create a new help command with reference to the command registry
    pub fn new(registry: SlashCommandRegistry) -> Self {
        Self { registry }
    }
}

impl SlashCommand for HelpCommand {
    fn name(&self) -> &str {
        "help"
    }

    fn description(&self) -> &str {
        "Show available commands"
    }

    fn help(&self) -> &str {
        "Usage: /help\n\nDisplays a list of all available slash commands with their descriptions."
    }

    fn execute(&self, _args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        let mut help_lines: Vec<String> = self
            .registry
            .names()
            .iter()
            .filter_map(|name| {
                self.registry.get(name).map(|cmd| {
                    format!("/{} - {}", cmd.name(), cmd.description())
                })
            })
            .collect();

        help_lines.sort();

        let help_text = format!(
            "Available commands:\n\n{}\n\nType /<command> to use a command.",
            help_lines.join("\n")
        );

        CommandOutput::insert(help_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::HeadingCommand;

    #[test]
    fn test_help_command_name() {
        let registry = SlashCommandRegistry::new();
        let cmd = HelpCommand::new(registry);
        assert_eq!(cmd.name(), "help");
    }

    #[test]
    fn test_help_command_description() {
        let registry = SlashCommandRegistry::new();
        let cmd = HelpCommand::new(registry);
        assert!(!cmd.description().is_empty());
    }
}
