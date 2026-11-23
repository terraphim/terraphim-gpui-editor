//! Terraphim GPUI Editor
//!
//! A GPUI-based desktop editor with intelligent autocomplete powered by
//! `terraphim_automata` and slash command support.

#![recursion_limit = "256"]

pub mod commands;
pub mod completion;
pub mod editor;
pub mod slash_command;

mod blocks;
mod document;
mod gitea_tasks;
mod indicators;
mod kg_records;
pub use kg_records::{markdown_kg_request, RoleSource};

#[cfg(test)]
mod test_helpers;

pub use completion::{CompletionConfig, TerraphimCompletionProvider};
pub use editor::TerraphimEditor;
pub use slash_command::{CommandOutput, SlashCommand, SlashCommandRegistry};

/// Return the alternative marks submitted during the most recent editor paint pass.
#[cfg(feature = "gdp")]
#[must_use]
pub fn headless_indicators(cx: &gpui::App) -> serde_json::Value {
    indicators::paint_snapshot(cx)
}

use thiserror::Error;

/// Errors that can occur in the editor
#[derive(Debug, Error)]
pub enum EditorError {
    #[error("failed to load autocomplete index: {0}")]
    IndexLoad(String),

    #[error("command not found: {0}")]
    CommandNotFound(String),

    #[error("invalid command arguments: {0}")]
    InvalidArguments(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Automata(#[from] terraphim_automata::TerraphimAutomataError),
}

/// Result type for editor operations
pub type Result<T> = std::result::Result<T, EditorError>;
