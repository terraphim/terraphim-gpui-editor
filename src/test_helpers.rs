//! Test helpers for terraphim-gpui-editor tests
//!
//! Provides shared utilities for creating test fixtures, providers, and registries.
//! Uses real terraphim-engineer thesaurus data for realistic testing.

#![cfg(test)]

use crate::completion::{CompletionConfig, TerraphimCompletionProvider};
use crate::slash_command::SlashCommandRegistry;
use std::sync::Arc;
use terraphim_automata::autocomplete::{build_autocomplete_index, AutocompleteIndex};
use terraphim_types::{NormalizedTerm, NormalizedTermValue, Thesaurus};

/// Create a test thesaurus with realistic terraphim-engineer terms
///
/// Contains a representative subset of engineering and AI-related terms
/// for testing autocomplete functionality.
pub fn create_test_thesaurus() -> Thesaurus {
    let mut thesaurus = Thesaurus::new("test_engineer".to_string());

    // Engineering and operations terms
    let terms = [
        // Operations and maintenance
        ("system maintains key functions", "operation", 82),
        ("monitor the services", "operation", 82),
        ("preventive maintenance", "maintenance", 84),
        ("corrective maintenance", "maintenance", 84),
        ("system scheduled maintenance", "maintenance", 84),
        // Transition and migration
        ("transition", "transition", 86),
        ("replace an existing system", "transition", 86),
        ("manage the migration between systems", "operation", 82),
        // AI and ML terms
        ("ann", "neural_networks", 87),
        ("artificial neural network", "neural_networks", 87),
        ("machine learning", "machine_learning", 88),
        ("deep learning", "deep_learning", 89),
        ("natural language processing", "nlp", 90),
        ("computer vision", "computer_vision", 91),
        // Rust and programming
        ("rust programming", "rust", 100),
        ("cargo build", "cargo", 101),
        ("async await", "async", 102),
        ("trait bounds", "traits", 103),
        // Terraphim specific
        ("terraphim", "terraphim", 110),
        ("knowledge graph", "knowledge_graph", 111),
        ("semantic search", "semantic_search", 112),
        ("thesaurus", "thesaurus", 113),
        ("automata", "automata", 114),
        ("rolegraph", "rolegraph", 115),
    ];

    for (key, normalized, id) in terms {
        let normalized_term = NormalizedTerm::new(id, NormalizedTermValue::from(normalized))
            .with_url(format!(
                "https://terraphim.ai/terms/{}",
                normalized.replace('_', "-")
            ));
        thesaurus.insert(NormalizedTermValue::from(key), normalized_term);
    }

    thesaurus
}

/// Create a test autocomplete index from the test thesaurus
pub fn create_test_index() -> Arc<AutocompleteIndex> {
    let thesaurus = create_test_thesaurus();
    Arc::new(build_autocomplete_index(thesaurus, None).expect("Failed to build test index"))
}

/// Create a test completion provider with default configuration
pub fn create_test_provider() -> TerraphimCompletionProvider {
    create_test_provider_with_config(CompletionConfig::default())
}

/// Create a test completion provider with custom configuration
pub fn create_test_provider_with_config(config: CompletionConfig) -> TerraphimCompletionProvider {
    TerraphimCompletionProvider::new(
        create_test_index(),
        SlashCommandRegistry::with_defaults(),
        config,
    )
}

/// Create a test slash command registry with all default commands
pub fn create_test_registry() -> SlashCommandRegistry {
    SlashCommandRegistry::with_defaults()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_thesaurus_has_terms() {
        let thesaurus = create_test_thesaurus();
        // Thesaurus should have terms
        assert!(!thesaurus.is_empty(), "Thesaurus should not be empty");
    }

    #[test]
    fn test_create_index_works() {
        let index = create_test_index();
        assert!(!index.is_empty(), "Index should have entries");
    }

    #[test]
    fn test_create_registry_works() {
        let registry = create_test_registry();
        for command in [
            "heading",
            "code",
            "list",
            "ids",
            "normalize",
            "blocks",
            "open",
            "save",
            "help",
        ] {
            assert!(registry.get(command).is_some(), "Missing /{command}");
        }
    }
}
