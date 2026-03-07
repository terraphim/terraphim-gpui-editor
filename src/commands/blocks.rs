//! /blocks command - toggle block sidebar

use crate::slash_command::{CommandOutput, EditorAction, SlashCommand};
use gpui::{App, Window};

pub struct BlocksCommand;

impl SlashCommand for BlocksCommand {
    fn name(&self) -> &str {
        "blocks"
    }

    fn description(&self) -> &str {
        "Toggle block sidebar"
    }

    fn help(&self) -> &str {
        "Usage: /blocks\n\nShow or hide the block sidebar."
    }

    fn execute(&self, _args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        CommandOutput::action(EditorAction::ToggleBlocks)
    }
}
