//! Terraphim GPUI Editor
//!
//! A GPUI-based desktop editor with intelligent autocomplete powered by
//! terraphim_automata and slash command support.

pub mod commands;
pub mod completion;
pub mod editor;
pub mod slash_command;

pub use completion::{CompletionConfig, TerraphimCompletionProvider};
pub use editor::TerraphimEditor;
pub use slash_command::{CommandOutput, SlashCommand, SlashCommandRegistry};

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
