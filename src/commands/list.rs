//! /list command - Insert list item

use crate::slash_command::{CommandOutput, SlashCommand};
use gpui::{App, Window};

/// Inserts a markdown list item (bullet or numbered)
pub struct ListCommand;

impl SlashCommand for ListCommand {
    fn name(&self) -> &'static str {
        "list"
    }

    fn description(&self) -> &'static str {
        "Insert list item"
    }

    fn help(&self) -> &'static str {
        "Usage: /list [type]\n\n\
         Inserts a markdown list item.\n\
         Type can be 'bullet' (default) or 'numbered'.\n\n\
         Examples:\n\
         /list          -> - \n\
         /list bullet   -> - \n\
         /list numbered -> 1. "
    }

    fn accepts_arguments(&self) -> bool {
        true
    }

    fn execute(&self, args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        let list_type = args.first().map_or("bullet", String::as_str);

        let prefix = match list_type {
            "numbered" | "number" | "num" | "1" => "1. ",
            _ => "- ",
        };

        CommandOutput::insert(prefix.to_string())
    }

    fn complete_argument(&self, partial: &str, _window: &mut Window, _cx: &mut App) -> Vec<String> {
        let options = ["bullet", "numbered"];
        options
            .iter()
            .filter(|o| o.starts_with(partial))
            .map(std::string::ToString::to_string)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_command_name() {
        assert_eq!(ListCommand.name(), "list");
    }

    #[test]
    fn test_list_accepts_arguments() {
        assert!(ListCommand.accepts_arguments());
    }
}
