//! /heading command - Insert markdown heading

use crate::slash_command::{CommandOutput, SlashCommand};
use gpui::{App, Window};

/// Inserts a markdown heading with configurable level (1-6)
pub struct HeadingCommand;

impl SlashCommand for HeadingCommand {
    fn name(&self) -> &str {
        "heading"
    }

    fn description(&self) -> &str {
        "Insert heading (# to ######)"
    }

    fn help(&self) -> &str {
        "Usage: /heading [level]\n\n\
         Inserts a markdown heading.\n\
         Level can be 1-6 (default: 1).\n\n\
         Examples:\n\
         /heading     -> # \n\
         /heading 2   -> ## \n\
         /heading 3   -> ### "
    }

    fn accepts_arguments(&self) -> bool {
        true
    }

    fn execute(&self, args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        let level = args
            .first()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 6);

        let prefix = "#".repeat(level);
        CommandOutput::insert(format!("{} ", prefix))
    }

    fn complete_argument(
        &self,
        partial: &str,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Vec<String> {
        let levels = ["1", "2", "3", "4", "5", "6"];
        levels
            .iter()
            .filter(|l| l.starts_with(partial))
            .map(|s| s.to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading_command_name() {
        assert_eq!(HeadingCommand.name(), "heading");
    }

    #[test]
    fn test_heading_accepts_arguments() {
        assert!(HeadingCommand.accepts_arguments());
    }

    // Note: test_complete_argument requires GPUI context (Window/App)
    // which can't be easily created in unit tests. Test via story example instead.
}
