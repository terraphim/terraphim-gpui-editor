//! /open command - open a markdown file

use crate::slash_command::{CommandOutput, EditorAction, SlashCommand};
use gpui::{App, Window};

pub struct OpenCommand;

impl SlashCommand for OpenCommand {
    fn name(&self) -> &str {
        "open"
    }

    fn description(&self) -> &str {
        "Open a markdown file from disk"
    }

    fn help(&self) -> &str {
        "Usage: /open <path>\n\nLoads the file and normalizes Terraphim block IDs."
    }

    fn requires_argument(&self) -> bool {
        true
    }

    fn accepts_arguments(&self) -> bool {
        true
    }

    fn execute(&self, args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        if args.is_empty() {
            return CommandOutput::message("Usage: /open <path>");
        }

        let path = args.join(" ");
        CommandOutput::action(EditorAction::OpenFile(path))
    }
}
