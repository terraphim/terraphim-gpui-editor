//! Built-in slash commands
//!
//! This module provides the default set of slash commands available in the editor.

mod code;
mod heading;
mod help;
mod blocks;
mod ids;
mod list;
mod open;
mod save;

pub use blocks::BlocksCommand;
pub use code::CodeCommand;
pub use heading::HeadingCommand;
pub use help::HelpCommand;
pub use ids::{IdsCommand, NormalizeCommand};
pub use list::ListCommand;
pub use open::OpenCommand;
pub use save::SaveCommand;
