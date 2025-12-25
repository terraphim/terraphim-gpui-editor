# Implementation Plan: terraphim-gpui-editor

**Status**: Draft
**Research Doc**: `.docs/research-document.md`
**Author**: Claude
**Date**: 2025-12-25
**Estimated Effort**: 8-12 hours

## Overview

### Summary
Build a native desktop text editor using GPUI framework with intelligent autocomplete powered by `terraphim_automata` and slash command support. The editor provides FST-based prefix search, Jaro-Winkler fuzzy matching, and a command palette for quick actions.

### Approach
- Use gpui-component's `Input` with custom `CompletionProvider`
- Integrate `terraphim_automata` directly (no IPC) for sub-millisecond search
- Implement slash commands as a registry with pluggable command trait
- Debounce autocomplete at 200ms to balance responsiveness and efficiency

### Scope
**In Scope:**
- Multi-line text editor with GPUI
- Terraphim autocomplete integration
- Slash command system (/, filter, execute)
- Tab/Shift+Tab rotation through suggestions
- Ctrl+Space to reopen completions
- Serialized index loading for fast startup

**Out of Scope:**
- Syntax highlighting
- File I/O operations
- Multiple buffers/tabs
- Undo/redo history
- Remote thesaurus loading

## Architecture

### Component Diagram
```
┌─────────────────────────────────────────────────────────────┐
│                     TerraphimEditor                          │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                    InputState                           ││
│  │  ┌─────────────┐  ┌──────────────────────────────────┐ ││
│  │  │   Buffer    │  │     TerraphimCompletionProvider  │ ││
│  │  │  (Rope)     │  │  ┌────────────────────────────┐  │ ││
│  │  └─────────────┘  │  │   AutocompleteIndex (FST)  │  │ ││
│  │                   │  └────────────────────────────┘  │ ││
│  │                   │  ┌────────────────────────────┐  │ ││
│  │                   │  │   SlashCommandRegistry     │  │ ││
│  │                   │  └────────────────────────────┘  │ ││
│  │                   └──────────────────────────────────┘ ││
│  └─────────────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────────────┐│
│  │                  CompletionMenu                          ││
│  │  [Suggestion 1] ← Tab/Shift+Tab rotation                ││
│  │  [Suggestion 2]                                         ││
│  │  [Suggestion 3]                                         ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

### Data Flow
```
Keystroke → InputState.on_change() → Debounce(200ms)
                                          ↓
                               TerraphimCompletionProvider
                                          ↓
                    ┌─────────────────────┴─────────────────────┐
                    ↓                                           ↓
         "/" prefix detected?                          Regular text
                    ↓                                           ↓
         SlashCommandRegistry.filter()           AutocompleteIndex.search()
                    ↓                                           ↓
         Command suggestions                       Term suggestions
                    ↓                                           ↓
                    └─────────────────────┬─────────────────────┘
                                          ↓
                               CompletionResponse
                                          ↓
                               CompletionMenu (UI)
                                          ↓
                    Tab/Enter accepts → Apply to buffer
```

### Key Design Decisions
| Decision | Rationale | Alternatives Rejected |
|----------|-----------|----------------------|
| Direct terraphim_automata integration | Sub-ms latency, no IPC overhead | MCP server (adds network latency) |
| gpui-component Input | Built-in completion support | Custom text editor (too complex) |
| 200ms debounce | Balance UX and performance | No debounce (too many queries) |
| Trait-based SlashCommand | Extensibility, clean API | Enum (not extensible) |
| Serialized index at startup | Fast cold start | Build index on demand (slow) |

## File Changes

### New Files
| File | Purpose |
|------|---------|
| `Cargo.toml` | Project manifest with dependencies |
| `src/main.rs` | Application entry point |
| `src/lib.rs` | Library root, re-exports |
| `src/editor.rs` | TerraphimEditor view component |
| `src/completion.rs` | TerraphimCompletionProvider implementation |
| `src/slash_command.rs` | SlashCommand trait and registry |
| `src/commands/mod.rs` | Built-in commands module |
| `src/commands/help.rs` | /help command |
| `src/commands/heading.rs` | /heading command |
| `src/commands/code.rs` | /code command |
| `src/theme.rs` | Theme configuration |
| `assets/` | Icons and fonts (copy from gpui-component) |

### Modified Files
None - new project

### Deleted Files
None - new project

## API Design

### Core Types

```rust
// src/lib.rs
pub mod completion;
pub mod commands;
pub mod editor;
pub mod slash_command;
pub mod theme;

pub use completion::TerraphimCompletionProvider;
pub use editor::TerraphimEditor;
pub use slash_command::{SlashCommand, SlashCommandRegistry};
```

### Editor Component

```rust
// src/editor.rs
use gpui::*;
use gpui_component::input::{Input, InputState, InputEvent};
use std::sync::Arc;

/// Main editor view with autocomplete and slash command support
pub struct TerraphimEditor {
    /// Input state managing the text buffer
    input_state: Entity<InputState>,
    /// Subscriptions kept alive with the editor
    _subscriptions: Vec<Subscription>,
}

impl TerraphimEditor {
    /// Create a new editor with the given autocomplete index
    ///
    /// # Arguments
    /// * `index` - Pre-built autocomplete index from terraphim_automata
    /// * `commands` - Registry of available slash commands
    /// * `window` - GPUI window reference
    /// * `cx` - GPUI context
    pub fn new(
        index: Arc<AutocompleteIndex>,
        commands: SlashCommandRegistry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self;
}

impl Render for TerraphimEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
}
```

### Completion Provider

```rust
// src/completion.rs
use gpui::*;
use gpui_component::input::{CompletionProvider, InputState};
use lsp_types::{CompletionContext, CompletionItem, CompletionResponse};
use ropey::Rope;
use std::sync::Arc;
use terraphim_automata::autocomplete::{AutocompleteIndex, autocomplete_search, fuzzy_autocomplete_search};

use crate::slash_command::SlashCommandRegistry;

/// Configuration for completion behavior
#[derive(Debug, Clone)]
pub struct CompletionConfig {
    /// Maximum suggestions to show
    pub max_suggestions: usize,
    /// Minimum prefix length to trigger autocomplete
    pub min_prefix_length: usize,
    /// Minimum similarity for fuzzy matching (0.0 - 1.0)
    pub fuzzy_threshold: f64,
    /// Debounce delay in milliseconds
    pub debounce_ms: u64,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            max_suggestions: 5,
            min_prefix_length: 2,
            fuzzy_threshold: 0.6,
            debounce_ms: 200,
        }
    }
}

/// Completion provider integrating terraphim_automata and slash commands
pub struct TerraphimCompletionProvider {
    /// Autocomplete index for term suggestions
    index: Arc<AutocompleteIndex>,
    /// Registry of slash commands
    commands: SlashCommandRegistry,
    /// Configuration
    config: CompletionConfig,
}

impl TerraphimCompletionProvider {
    /// Create a new completion provider
    pub fn new(
        index: Arc<AutocompleteIndex>,
        commands: SlashCommandRegistry,
        config: CompletionConfig,
    ) -> Self;

    /// Check if we're in slash command mode
    fn is_slash_command_context(&self, text: &str, offset: usize) -> Option<(usize, String)>;

    /// Get slash command completions
    fn complete_slash_commands(&self, query: &str) -> Vec<CompletionItem>;

    /// Get term autocomplete suggestions
    fn complete_terms(&self, prefix: &str, use_fuzzy: bool) -> Vec<CompletionItem>;
}

impl CompletionProvider for TerraphimCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut Context<InputState>,
    ) -> Task<Result<CompletionResponse>>;

    fn is_completion_trigger(
        &self,
        _offset: usize,
        new_text: &str,
        _cx: &mut Context<InputState>,
    ) -> bool;
}
```

### Slash Command System

```rust
// src/slash_command.rs
use gpui::*;
use std::collections::HashMap;
use std::sync::Arc;

/// Result of executing a slash command
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Text to insert at cursor
    pub insert_text: Option<String>,
    /// Whether to close the completion menu
    pub close_menu: bool,
    /// Optional message to display
    pub message: Option<String>,
}

/// Trait for implementing slash commands
pub trait SlashCommand: Send + Sync {
    /// Command name (without the leading /)
    fn name(&self) -> &str;

    /// Short description for completion menu
    fn description(&self) -> &str;

    /// Detailed help text
    fn help(&self) -> &str {
        self.description()
    }

    /// Whether this command requires arguments
    fn requires_argument(&self) -> bool {
        false
    }

    /// Whether this command accepts arguments
    fn accepts_arguments(&self) -> bool {
        self.requires_argument()
    }

    /// Execute the command
    ///
    /// # Arguments
    /// * `args` - Command arguments (empty if none)
    /// * `window` - GPUI window
    /// * `cx` - GPUI context
    fn execute(
        &self,
        args: &[String],
        window: &mut Window,
        cx: &mut App,
    ) -> CommandOutput;

    /// Provide argument completions
    fn complete_argument(
        &self,
        _partial: &str,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Vec<String> {
        Vec::new()
    }
}

/// Registry of available slash commands
#[derive(Clone, Default)]
pub struct SlashCommandRegistry {
    commands: HashMap<String, Arc<dyn SlashCommand>>,
}

impl SlashCommandRegistry {
    /// Create a new empty registry
    pub fn new() -> Self;

    /// Register a slash command
    pub fn register(&mut self, command: impl SlashCommand + 'static);

    /// Get a command by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn SlashCommand>>;

    /// Get all command names
    pub fn names(&self) -> Vec<&str>;

    /// Filter commands by prefix
    pub fn filter(&self, prefix: &str) -> Vec<Arc<dyn SlashCommand>>;

    /// Create registry with default commands
    pub fn with_defaults() -> Self;
}
```

### Built-in Commands

```rust
// src/commands/mod.rs
mod help;
mod heading;
mod code;

pub use help::HelpCommand;
pub use heading::HeadingCommand;
pub use code::CodeCommand;

// src/commands/help.rs
use crate::slash_command::{SlashCommand, CommandOutput, SlashCommandRegistry};

/// /help - Show available commands
pub struct HelpCommand {
    registry: SlashCommandRegistry,
}

impl HelpCommand {
    pub fn new(registry: SlashCommandRegistry) -> Self {
        Self { registry }
    }
}

impl SlashCommand for HelpCommand {
    fn name(&self) -> &str { "help" }
    fn description(&self) -> &str { "Show available commands" }

    fn execute(&self, _args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        let help_text = self.registry.names()
            .iter()
            .map(|name| format!("/{} - {}", name, self.registry.get(name).unwrap().description()))
            .collect::<Vec<_>>()
            .join("\n");

        CommandOutput {
            insert_text: Some(help_text),
            close_menu: true,
            message: None,
        }
    }
}

// src/commands/heading.rs
/// /heading [level] - Insert markdown heading
pub struct HeadingCommand;

impl SlashCommand for HeadingCommand {
    fn name(&self) -> &str { "heading" }
    fn description(&self) -> &str { "Insert heading (# to ######)" }
    fn accepts_arguments(&self) -> bool { true }

    fn execute(&self, args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        let level = args.get(0)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 6);

        let prefix = "#".repeat(level);
        CommandOutput {
            insert_text: Some(format!("{} ", prefix)),
            close_menu: true,
            message: None,
        }
    }

    fn complete_argument(&self, _partial: &str, _window: &mut Window, _cx: &mut App) -> Vec<String> {
        vec!["1", "2", "3", "4", "5", "6"].into_iter().map(String::from).collect()
    }
}

// src/commands/code.rs
/// /code [lang] - Insert code block
pub struct CodeCommand;

impl SlashCommand for CodeCommand {
    fn name(&self) -> &str { "code" }
    fn description(&self) -> &str { "Insert code block" }
    fn accepts_arguments(&self) -> bool { true }

    fn execute(&self, args: &[String], _window: &mut Window, _cx: &mut App) -> CommandOutput {
        let lang = args.get(0).map(|s| s.as_str()).unwrap_or("");
        CommandOutput {
            insert_text: Some(format!("```{}\n\n```", lang)),
            close_menu: true,
            message: None,
        }
    }

    fn complete_argument(&self, partial: &str, _window: &mut Window, _cx: &mut App) -> Vec<String> {
        let langs = ["rust", "python", "javascript", "typescript", "go", "bash"];
        langs.iter()
            .filter(|l| l.starts_with(partial))
            .map(|s| s.to_string())
            .collect()
    }
}
```

### Error Types

```rust
// src/lib.rs (add to existing)
use thiserror::Error;

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
}
```

## Test Strategy

### Unit Tests
| Test | Location | Purpose |
|------|----------|---------|
| `test_completion_provider_slash_detection` | `completion.rs` | Detect / prefix correctly |
| `test_completion_provider_term_search` | `completion.rs` | FST search returns results |
| `test_completion_provider_fuzzy_search` | `completion.rs` | Jaro-Winkler finds typos |
| `test_slash_command_registry_filter` | `slash_command.rs` | Prefix filtering works |
| `test_heading_command_levels` | `commands/heading.rs` | All heading levels work |
| `test_code_command_languages` | `commands/code.rs` | Language completion works |

### Integration Tests
| Test | Location | Purpose |
|------|----------|---------|
| `test_editor_autocomplete_flow` | `tests/editor.rs` | Full autocomplete UX |
| `test_editor_slash_command_flow` | `tests/editor.rs` | Slash command execution |
| `test_editor_keyboard_shortcuts` | `tests/editor.rs` | Tab rotation, Ctrl+Space |

### Example Test Implementations

```rust
// src/completion.rs
#[cfg(test)]
mod tests {
    use super::*;
    use terraphim_automata::autocomplete::build_autocomplete_index;
    use terraphim_types::Thesaurus;

    fn create_test_index() -> Arc<AutocompleteIndex> {
        let mut thesaurus = Thesaurus::new("Test".to_string());
        // Add test terms
        Arc::new(build_autocomplete_index(thesaurus, None).unwrap())
    }

    #[test]
    fn test_is_slash_command_context() {
        let provider = TerraphimCompletionProvider::new(
            create_test_index(),
            SlashCommandRegistry::with_defaults(),
            CompletionConfig::default(),
        );

        // Should detect slash at start of line
        assert!(provider.is_slash_command_context("/he", 3).is_some());

        // Should not detect slash in middle of word
        assert!(provider.is_slash_command_context("hello/world", 11).is_none());
    }

    #[test]
    fn test_complete_terms_prefix() {
        let provider = TerraphimCompletionProvider::new(
            create_test_index(),
            SlashCommandRegistry::default(),
            CompletionConfig::default(),
        );

        let results = provider.complete_terms("mach", false);
        assert!(!results.is_empty());
    }
}

// src/slash_command.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_filter() {
        let registry = SlashCommandRegistry::with_defaults();

        let filtered = registry.filter("he");
        assert!(filtered.iter().any(|c| c.name() == "help" || c.name() == "heading"));
    }
}
```

## Implementation Steps

### Step 1: Project Setup
**Files:** `Cargo.toml`, `src/main.rs`, `src/lib.rs`
**Description:** Create project structure with dependencies
**Tests:** `cargo build` compiles successfully
**Estimated:** 30 minutes

```toml
# Cargo.toml
[package]
name = "terraphim-gpui-editor"
version = "0.1.0"
edition = "2021"

[dependencies]
gpui = "0.2"
gpui-component = { path = "../terraphim-ai-gpui/tmp/gpui-component/crates/ui" }
gpui-component-assets = { path = "../terraphim-ai-gpui/tmp/gpui-component/crates/assets" }
terraphim_automata = { path = "../terraphim-ai/crates/terraphim_automata" }
terraphim_types = { path = "../terraphim-ai/crates/terraphim_types" }
lsp-types = "0.95"
ropey = "1.6"
anyhow = "1.0"
thiserror = "1.0"
log = "0.4"

[dev-dependencies]
env_logger = "0.11"
```

### Step 2: Slash Command Trait and Registry
**Files:** `src/slash_command.rs`
**Description:** Define SlashCommand trait and registry
**Tests:** Registry filter and lookup tests
**Dependencies:** Step 1
**Estimated:** 1 hour

### Step 3: Built-in Commands
**Files:** `src/commands/mod.rs`, `src/commands/help.rs`, `src/commands/heading.rs`, `src/commands/code.rs`
**Description:** Implement /help, /heading, /code commands
**Tests:** Command execution tests
**Dependencies:** Step 2
**Estimated:** 1 hour

### Step 4: Completion Provider
**Files:** `src/completion.rs`
**Description:** Implement TerraphimCompletionProvider with slash detection and term search
**Tests:** Slash detection, term search, fuzzy search tests
**Dependencies:** Step 2, Step 3
**Estimated:** 2 hours

### Step 5: Editor Component
**Files:** `src/editor.rs`
**Description:** Create TerraphimEditor with Input and completion integration
**Tests:** Manual testing in GPUI window
**Dependencies:** Step 4
**Estimated:** 2 hours

### Step 6: Theme and Styling
**Files:** `src/theme.rs`, `assets/`
**Description:** Configure theme colors and copy required assets
**Tests:** Visual inspection
**Dependencies:** Step 5
**Estimated:** 30 minutes

### Step 7: Main Application
**Files:** `src/main.rs`
**Description:** Wire everything together, load index, create window
**Tests:** Full application runs
**Dependencies:** Step 6
**Estimated:** 1 hour

### Step 8: Keyboard Shortcuts
**Files:** `src/editor.rs` (update)
**Description:** Add Tab rotation, Ctrl+Space, Escape handling
**Tests:** Keyboard interaction tests
**Dependencies:** Step 7
**Estimated:** 1 hour

### Step 9: Integration Tests
**Files:** `tests/editor.rs`
**Description:** Write comprehensive integration tests
**Tests:** All integration test scenarios
**Dependencies:** Step 8
**Estimated:** 1 hour

## Rollback Plan

If issues discovered:
1. Revert to previous commit
2. Disable completion provider (use plain Input)

No feature flags needed for initial implementation.

## Dependencies

### New Dependencies
| Crate | Version | Justification |
|-------|---------|---------------|
| gpui | 0.2 | UI framework |
| gpui-component | local | UI components with completion |
| lsp-types | 0.95 | CompletionItem types |
| ropey | 1.6 | Rope text buffer |
| anyhow | 1.0 | Error handling |
| thiserror | 1.0 | Error types |

### Existing Dependencies (from terraphim-ai)
| Crate | Used For |
|-------|----------|
| terraphim_automata | Autocomplete index and search |
| terraphim_types | Thesaurus and NormalizedTerm |

## Performance Considerations

### Expected Performance
| Metric | Target | Measurement |
|--------|--------|-------------|
| Keystroke latency | < 16ms | Manual profiling |
| Autocomplete latency | < 200ms (debounced) | Logging |
| FST search | < 1ms | Benchmark |
| Index load | < 500ms | Logging |

### Benchmarks to Add
```rust
// benches/completion.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_autocomplete_search(c: &mut Criterion) {
    let index = load_test_index();
    c.bench_function("autocomplete_search_5", |b| {
        b.iter(|| autocomplete_search(&index, "mach", Some(5)))
    });
}

fn bench_fuzzy_search(c: &mut Criterion) {
    let index = load_test_index();
    c.bench_function("fuzzy_search_5", |b| {
        b.iter(|| fuzzy_autocomplete_search(&index, "machne", 0.6, Some(5)))
    });
}

criterion_group!(benches, bench_autocomplete_search, bench_fuzzy_search);
criterion_main!(benches);
```

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Confirm gpui-component path | Pending | Alex |
| Test thesaurus file location | Pending | Alex |
| Decide on Tab vs Arrow key rotation | Resolved: Tab | Research |

## Approval

- [ ] Technical review complete
- [ ] Test strategy approved
- [ ] Performance targets agreed
- [ ] Human approval received
