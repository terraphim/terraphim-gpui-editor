//! /ids and /normalize commands - ensure Terraphim block IDs exist

use crate::slash_command::{CommandOutput, EditorAction, SlashCommand};
use gpui::{App, Window};

pub struct IdsCommand;

impl SlashCommand for IdsCommand {
    fn name(&self) -> &str {
        "ids"
    }

    fn description(&self) -> &str {
        "Insert missing Terraphim block IDs"
    }

    fn help(&self) -> &str {
        "Usage: /ids\n\nEnsures every list item and paragraph has a Terraphim block id."
    }

    fn execute(&self, _args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        CommandOutput::action(EditorAction::NormalizeIds)
    }
}

pub struct NormalizeCommand;

impl SlashCommand for NormalizeCommand {
    fn name(&self) -> &str {
        "normalize"
    }

    fn description(&self) -> &str {
        "Normalize markdown and insert missing block IDs"
    }

    fn help(&self) -> &str {
        "Usage: /normalize\n\nAlias for /ids."
    }

    fn execute(&self, _args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        CommandOutput::action(EditorAction::NormalizeIds)
    }
}
