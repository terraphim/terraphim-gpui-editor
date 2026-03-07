//! /save command - save current markdown to disk

use crate::slash_command::{CommandOutput, EditorAction, SlashCommand};
use gpui::{App, Window};

pub struct SaveCommand;

impl SlashCommand for SaveCommand {
    fn name(&self) -> &str {
        "save"
    }

    fn description(&self) -> &str {
        "Save markdown to disk (normalizes block IDs)"
    }

    fn help(&self) -> &str {
        "Usage: /save [path]\n\nSaves normalized markdown to the current file path or the provided path."
    }

    fn execute(&self, args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        let path = if args.is_empty() {
            None
        } else {
            Some(args.join(" "))
        };
        CommandOutput::action(EditorAction::SaveFile(path))
    }
}
