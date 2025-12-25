//! /code command - Insert code block

use crate::slash_command::{CommandOutput, SlashCommand};
use gpui::{App, Window};

/// Inserts a markdown code block with optional language
pub struct CodeCommand;

/// Common programming languages for code block completion
const LANGUAGES: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "go",
    "bash",
    "shell",
    "json",
    "yaml",
    "toml",
    "sql",
    "html",
    "css",
    "markdown",
    "c",
    "cpp",
    "java",
    "kotlin",
    "swift",
    "ruby",
];

impl SlashCommand for CodeCommand {
    fn name(&self) -> &str {
        "code"
    }

    fn description(&self) -> &str {
        "Insert code block"
    }

    fn help(&self) -> &str {
        "Usage: /code [language]\n\n\
         Inserts a markdown code block.\n\
         Optionally specify a language for syntax highlighting.\n\n\
         Examples:\n\
         /code        -> ```\\n\\n```\n\
         /code rust   -> ```rust\\n\\n```\n\
         /code python -> ```python\\n\\n```"
    }

    fn accepts_arguments(&self) -> bool {
        true
    }

    fn execute(&self, args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        let lang = args.first().map(String::as_str).unwrap_or("");
        CommandOutput::insert(format!("```{}\n\n```", lang))
    }

    fn complete_argument(
        &self,
        partial: &str,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Vec<String> {
        let partial_lower = partial.to_lowercase();
        LANGUAGES
            .iter()
            .filter(|l| l.starts_with(&partial_lower))
            .map(|s| s.to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_command_name() {
        assert_eq!(CodeCommand.name(), "code");
    }

    #[test]
    fn test_code_accepts_arguments() {
        assert!(CodeCommand.accepts_arguments());
    }

    #[test]
    fn test_languages_not_empty() {
        assert!(!LANGUAGES.is_empty());
        assert!(LANGUAGES.contains(&"rust"));
    }
}
