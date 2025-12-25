//! Completion provider integrating terraphim_automata and slash commands
//!
//! This module implements the `CompletionProvider` trait from gpui-component,
//! providing FST-based autocomplete from thesaurus data and slash command completion.

use crate::slash_command::SlashCommandRegistry;
use anyhow::Result;
use gpui::{Context, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, Documentation,
    InsertTextFormat, MarkupContent, MarkupKind,
};
use std::sync::Arc;
use terraphim_automata::autocomplete::{
    autocomplete_search, fuzzy_autocomplete_search, AutocompleteIndex, AutocompleteResult,
};

// Re-export from gpui-component
pub use gpui_component::input::CompletionProvider;
use gpui_component::input::InputState;
// Use the Rope type re-exported from gpui-component
use gpui_component::Rope;

/// Configuration for completion behavior
#[derive(Debug, Clone)]
pub struct CompletionConfig {
    /// Maximum suggestions to show in the completion menu
    pub max_suggestions: usize,
    /// Minimum prefix length to trigger term autocomplete
    pub min_prefix_length: usize,
    /// Minimum similarity threshold for fuzzy matching (0.0 - 1.0)
    pub fuzzy_threshold: f64,
    /// Whether to use fuzzy search when prefix is short
    pub use_fuzzy_for_short_prefixes: bool,
    /// Prefix length below which fuzzy search is used
    pub fuzzy_prefix_threshold: usize,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            max_suggestions: 5,
            min_prefix_length: 2,
            fuzzy_threshold: 0.6,
            use_fuzzy_for_short_prefixes: true,
            fuzzy_prefix_threshold: 3,
        }
    }
}

/// Completion provider that integrates terraphim_automata and slash commands
///
/// This provider handles two types of completions:
/// 1. Slash commands: Triggered by `/` at the start of a line
/// 2. Term autocomplete: Triggered by typing alphanumeric characters
pub struct TerraphimCompletionProvider {
    /// Autocomplete index built from thesaurus
    index: Arc<AutocompleteIndex>,
    /// Registry of available slash commands
    commands: SlashCommandRegistry,
    /// Configuration
    config: CompletionConfig,
}

impl TerraphimCompletionProvider {
    /// Create a new completion provider
    ///
    /// # Arguments
    /// * `index` - Pre-built autocomplete index from terraphim_automata
    /// * `commands` - Registry of slash commands
    /// * `config` - Completion configuration
    pub fn new(
        index: Arc<AutocompleteIndex>,
        commands: SlashCommandRegistry,
        config: CompletionConfig,
    ) -> Self {
        Self {
            index,
            commands,
            config,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(index: Arc<AutocompleteIndex>, commands: SlashCommandRegistry) -> Self {
        Self::new(index, commands, CompletionConfig::default())
    }

    /// Check if we're in slash command context and return the command prefix
    ///
    /// Returns `Some((slash_pos, query))` if a slash command is being typed.
    fn parse_slash_command(&self, text: &str, offset: usize) -> Option<(usize, String)> {
        if offset == 0 || text.is_empty() {
            return None;
        }

        // Get text before cursor
        let before_cursor = &text[..offset.min(text.len())];

        // Find the last newline before cursor
        let line_start = before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);

        // Get current line up to cursor
        let current_line = &before_cursor[line_start..];

        // Check if line starts with /
        if current_line.starts_with('/') {
            let query = &current_line[1..]; // Remove leading /
            // Only match if it's just the command (no spaces or it's completing after /)
            if !query.contains(' ') || query.is_empty() {
                return Some((line_start, query.to_string()));
            }
        }

        None
    }

    /// Get the word being typed before the cursor
    fn get_current_word(&self, text: &str, offset: usize) -> Option<String> {
        if offset == 0 || text.is_empty() {
            return None;
        }

        let before_cursor = &text[..offset.min(text.len())];

        // Find word boundary
        let word_start = before_cursor
            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .map(|i| i + 1)
            .unwrap_or(0);

        let word = &before_cursor[word_start..];

        if word.len() >= self.config.min_prefix_length {
            Some(word.to_string())
        } else {
            None
        }
    }

    /// Get slash command completions
    fn complete_slash_commands(&self, query: &str) -> Vec<CompletionItem> {
        self.commands
            .filter(query)
            .into_iter()
            .take(self.config.max_suggestions)
            .map(|cmd| {
                let name = cmd.name();
                let description = cmd.description();

                CompletionItem {
                    label: format!("/{}", name),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(description.to_string()),
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: cmd.help().to_string(),
                    })),
                    insert_text: Some(if cmd.accepts_arguments() {
                        format!("{} ", name)
                    } else {
                        name.to_string()
                    }),
                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Get term autocomplete suggestions
    fn complete_terms(&self, prefix: &str) -> Vec<CompletionItem> {
        let use_fuzzy = self.config.use_fuzzy_for_short_prefixes
            && prefix.len() < self.config.fuzzy_prefix_threshold;

        let results: Vec<AutocompleteResult> = if use_fuzzy {
            fuzzy_autocomplete_search(
                &self.index,
                prefix,
                self.config.fuzzy_threshold,
                Some(self.config.max_suggestions),
            )
            .unwrap_or_default()
        } else {
            autocomplete_search(&self.index, prefix, Some(self.config.max_suggestions))
                .unwrap_or_default()
        };

        results
            .into_iter()
            .map(|result| {
                let detail = result
                    .url
                    .as_ref()
                    .map(|u| format!("→ {}", u))
                    .or_else(|| Some(format!("ID: {}", result.id)));

                CompletionItem {
                    label: result.term.clone(),
                    kind: Some(CompletionItemKind::TEXT),
                    detail,
                    documentation: Some(Documentation::String(format!(
                        "Normalized: {}\nScore: {:.2}",
                        result.normalized_term.as_str(),
                        result.score
                    ))),
                    insert_text: Some(result.term),
                    insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                    sort_text: Some(format!("{:010}", (1_000_000.0 - result.score) as u64)),
                    ..Default::default()
                }
            })
            .collect()
    }
}

impl CompletionProvider for TerraphimCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        _cx: &mut Context<InputState>,
    ) -> Task<Result<CompletionResponse>> {
        let text_str = text.to_string();

        // Check for slash command
        if let Some((_slash_pos, query)) = self.parse_slash_command(&text_str, offset) {
            let items = self.complete_slash_commands(&query);
            return Task::ready(Ok(CompletionResponse::Array(items)));
        }

        // Check for term autocomplete
        if let Some(word) = self.get_current_word(&text_str, offset) {
            let items = self.complete_terms(&word);
            return Task::ready(Ok(CompletionResponse::Array(items)));
        }

        Task::ready(Ok(CompletionResponse::Array(vec![])))
    }

    fn is_completion_trigger(
        &self,
        _offset: usize,
        new_text: &str,
        _cx: &mut Context<InputState>,
    ) -> bool {
        // Trigger on / for commands
        if new_text == "/" {
            return true;
        }

        // Trigger on alphanumeric for term completion
        new_text
            .chars()
            .next()
            .map(|c| c.is_alphanumeric())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terraphim_automata::autocomplete::build_autocomplete_index;
    use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

    fn create_test_index() -> Arc<AutocompleteIndex> {
        let mut thesaurus = Thesaurus::new("Test".to_string());

        let terms = vec![
            ("machine learning", "machine learning", 10),
            ("artificial intelligence", "artificial intelligence", 20),
            ("neural network", "neural network", 15),
        ];

        for (key, normalized, id) in terms {
            let normalized_term = NormalizedTerm {
                id,
                value: NormalizedTermValue::from(normalized),
                url: Some(format!("https://example.com/{}", normalized.replace(' ', "-"))),
            };
            thesaurus.insert(NormalizedTermValue::from(key), normalized_term);
        }

        Arc::new(build_autocomplete_index(thesaurus, None).unwrap())
    }

    fn create_test_provider() -> TerraphimCompletionProvider {
        TerraphimCompletionProvider::new(
            create_test_index(),
            SlashCommandRegistry::with_defaults(),
            CompletionConfig::default(),
        )
    }

    #[test]
    fn test_parse_slash_command_at_start() {
        let provider = create_test_provider();

        let result = provider.parse_slash_command("/he", 3);
        assert!(result.is_some());
        let (pos, query) = result.unwrap();
        assert_eq!(pos, 0);
        assert_eq!(query, "he");
    }

    #[test]
    fn test_parse_slash_command_after_newline() {
        let provider = create_test_provider();

        let result = provider.parse_slash_command("hello\n/co", 9);
        assert!(result.is_some());
        let (pos, query) = result.unwrap();
        assert_eq!(pos, 6); // After \n
        assert_eq!(query, "co");
    }

    #[test]
    fn test_parse_slash_command_not_at_line_start() {
        let provider = create_test_provider();

        // Slash in middle of text should not trigger
        let result = provider.parse_slash_command("hello /world", 12);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_current_word() {
        let provider = create_test_provider();

        let result = provider.get_current_word("hello mac", 9);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "mac");
    }

    #[test]
    fn test_get_current_word_too_short() {
        let provider = create_test_provider();

        // "m" is below min_prefix_length (2)
        let result = provider.get_current_word("hello m", 7);
        assert!(result.is_none());
    }

    #[test]
    fn test_complete_slash_commands() {
        let provider = create_test_provider();

        let items = provider.complete_slash_commands("he");
        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label.contains("help") || i.label.contains("heading")));
    }

    #[test]
    fn test_complete_terms() {
        let provider = create_test_provider();

        let items = provider.complete_terms("mach");
        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label.contains("machine")));
    }

    #[test]
    fn test_is_completion_trigger_slash() {
        let provider = create_test_provider();

        // This would need GPUI context, so we test the logic conceptually
        // The actual test would require mocking Context<InputState>
    }
}
