# Research Document: terraphim-gpui-editor

**Status**: Draft
**Author**: Claude
**Date**: 2025-12-25
**Reviewers**: Alex

## Executive Summary

Create a GPUI-based desktop editor for Terraphim AI with intelligent autocomplete powered by `terraphim_automata` crate and slash command support following Zed editor patterns. The editor will provide FST-based prefix search, fuzzy matching with Jaro-Winkler similarity, and command palette functionality.

## Problem Statement

### Description
Build a native desktop text editor using GPUI (Zed's UI framework) that integrates Terraphim's knowledge graph autocomplete for semantic text completion and slash commands for quick actions.

### Impact
- Users can leverage Terraphim's thesaurus-based autocomplete in a fast native editor
- Slash commands provide quick access to common operations
- FST-based search provides sub-millisecond autocomplete response times

### Success Criteria
- Editor responds to keystrokes with < 16ms latency (60fps)
- Autocomplete suggestions appear within 150-250ms of typing
- Tab/Shift+Tab rotation through 3-5 suggestions
- Slash commands trigger on `/` with filtered dropdown
- Integration with `terraphim_automata` for FST-based search

## Current State Analysis

### Existing Implementation

#### terraphim_automata crate (core autocomplete)
- **Location**: `terraphim-ai/crates/terraphim_automata/src/autocomplete.rs`
- **Provides**:
  - `AutocompleteIndex` - FST-based index with metadata lookup
  - `AutocompleteResult` - {term, normalized_term, id, url, score}
  - `autocomplete_search(index, prefix, limit)` - prefix matching
  - `fuzzy_autocomplete_search(index, prefix, min_similarity, limit)` - Jaro-Winkler fuzzy search
  - `serialize/deserialize_autocomplete_index` - persistence

#### gpui-component library (UI components)
- **Location**: `terraphim-ai-gpui/tmp/gpui-component/`
- **Key Components**:
  - `Input` + `InputState` - text input with event handling
  - `InputEvent::Change` - text change notifications
  - `CompletionProvider` trait - LSP-style completions
  - `PopupMenu` + `PopupMenuItem` - menus and command palette
  - Theming via `ActiveTheme`

#### Zed slash command system (reference implementation)
- **Location**: `terraphim-ai-gpui/tmp/zed/crates/assistant_slash_command/`
- **Key Patterns**:
  - `SlashCommand` trait - command definition
  - `SlashCommandWorkingSet` - command registry
  - `SlashCommandCompletionProvider` - integrates with editor completions
  - Fuzzy matching via `fuzzy::match_strings`

### Code Locations
| Component | Location | Purpose |
|-----------|----------|---------|
| AutocompleteIndex | `crates/terraphim_automata/src/autocomplete.rs:12` | FST index structure |
| autocomplete_search | `crates/terraphim_automata/src/autocomplete.rs:169` | Prefix search |
| fuzzy_autocomplete_search | `crates/terraphim_automata/src/autocomplete.rs:328` | Jaro-Winkler fuzzy |
| Input component | `gpui-component/crates/ui/src/input/` | Text input element |
| CompletionProvider | `gpui-component/crates/ui/src/input/completion.rs` | Completion trait |
| SlashCommand | `zed/crates/assistant_slash_command/src/assistant_slash_command.rs` | Command trait |

### Data Flow
```
User Input → InputState → CompletionProvider → AutocompleteIndex
                                                     ↓
                                          AutocompleteResult[]
                                                     ↓
                                        CompletionMenu (UI)
                                                     ↓
                                     Tab/Shift+Tab Rotation
```

### Integration Points
- `terraphim_automata::build_autocomplete_index(thesaurus)` for index creation
- `gpui_component::input::CompletionProvider` for completion integration
- `gpui::KeyBinding` for keyboard shortcuts

## Constraints

### Technical Constraints
- **GPUI 0.2 + gpui-component 0.4**: Must use compatible API versions
- **macOS primary target**: GPUI is macOS-first (Linux support experimental)
- **Single-threaded UI**: All UI updates on foreground thread
- **No blocking in UI**: Autocomplete must be async or very fast

### Business Constraints
- Standalone project: `terraphim-gpui-editor` separate from main workspace
- Reuse existing `terraphim_automata` crate

### Non-Functional Requirements
| Requirement | Target | Current |
|-------------|--------|---------|
| Keystroke latency | < 16ms | N/A |
| Autocomplete latency | < 250ms | FST: < 1ms |
| Memory (thesaurus) | < 50MB | Depends on thesaurus size |
| Suggestions shown | 3-5 | Configurable |

## Dependencies

### Internal Dependencies
| Dependency | Impact | Risk |
|------------|--------|------|
| terraphim_automata | Core autocomplete functionality | Low - stable API |
| terraphim_types | Type definitions | Low - stable |

### External Dependencies
| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| gpui | 0.2 | Medium - API changes | None |
| gpui-component | 0.4 | Medium - evolving | Build from source |
| fst | 0.4 | Low - stable | None |
| strsim | 0.11 | Low - stable | None |

## Risks and Unknowns

### Known Risks
| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| GPUI API instability | Medium | High | Pin versions, follow Zed patterns |
| gpui-component breaking changes | Medium | Medium | Build from local source |
| Thesaurus loading slow | Low | Medium | Async loading, serialize index |
| Completion menu positioning | Medium | Low | Follow gpui-component examples |

### Open Questions
1. Should autocomplete trigger on every keystroke or debounced? → Research: 150-250ms debounce recommended
2. How to handle slash commands with arguments? → Zed pattern: space after command triggers argument completion
3. Multi-line editor or single-line? → Start with multi-line for full editor experience

### Assumptions
1. User has thesaurus JSON available locally
2. macOS development environment
3. Rust toolchain with GPUI build requirements (Xcode, etc.)

## Research Findings

### Key Insights

1. **GPUI Completion Pattern**: Use `CompletionProvider` trait with `completions()` method returning `Task<Result<CompletionResponse>>`

2. **Debouncing**: Research recommends 150-250ms debounce for autocomplete to avoid UI thrash

3. **Rotation UX**: Tab cycles forward, Shift+Tab backward through 3-5 visible items - matches shell/IDE muscle memory

4. **Slash Commands**: `/` triggers command mode, filter by typed text, Enter executes

5. **Data Shape for Suggestions**:
```rust
struct SuggestionItem {
    id: String,
    item_type: SuggestionType, // Action | Command | Term
    label: String,
    detail: Option<String>,
    shortcut: Option<String>,
    score: f64,
    preview: Option<String>,
}
```

6. **Zed Pattern**: Separate `SlashCommand` trait from `SlashCommandCompletionProvider` - commands are registered in a `SlashCommandWorkingSet` and completions are provided by a wrapper that implements `CompletionProvider`

### Relevant Prior Art
- **Zed Editor**: Full GPUI implementation with slash commands and autocomplete
- **gpui-component input example**: Basic input with events
- **terraphim MCP server**: Exposes autocomplete via MCP tools

### Technical Spikes Needed
| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| GPUI minimal app | Verify build and run | 1 hour |
| Input with completion | Validate CompletionProvider | 2 hours |
| Integrate terraphim_automata | Test FST search in GPUI context | 2 hours |

## Recommendations

### Proceed/No-Proceed
**Proceed** - All components exist, clear patterns from Zed, stable autocomplete API in terraphim_automata.

### Scope Recommendations
1. **Phase 1**: Basic editor with Input, text editing, theming
2. **Phase 2**: Autocomplete integration with terraphim_automata
3. **Phase 3**: Slash command support with command palette

### Risk Mitigation Recommendations
1. Pin gpui and gpui-component versions
2. Build gpui-component from local source to control updates
3. Use async index loading to avoid blocking UI
4. Cache serialized index for fast startup

## Next Steps

If approved:
1. Create `terraphim-gpui-editor` Rust project structure
2. Set up dependencies (gpui, gpui-component, terraphim_automata)
3. Implement basic editor skeleton with Input
4. Add TerraphimCompletionProvider
5. Implement slash command registry
6. Add keyboard shortcuts (Tab rotation, Ctrl+Space, etc.)
7. Write tests for completion provider

## Appendix

### Reference Materials
- `/Users/alex/projects/terraphim/editors_research/editor_autocomplete_integration_options.md`
- `/Users/alex/projects/terraphim/editors_research/next_best_action_autocomplete.md`
- `/Users/alex/projects/terraphim/terraphim-ai-gpui/tmp/gpui-component/examples/input/`
- `/Users/alex/projects/terraphim/terraphim-ai-gpui/tmp/zed/crates/assistant_slash_command/`

### Key API Examples

#### AutocompleteIndex Usage
```rust
use terraphim_automata::{
    autocomplete::{build_autocomplete_index, autocomplete_search, fuzzy_autocomplete_search},
};
use terraphim_types::Thesaurus;

// Build index from thesaurus
let thesaurus = load_thesaurus(&path).await?;
let index = build_autocomplete_index(thesaurus, None)?;

// Prefix search
let results = autocomplete_search(&index, "mach", Some(5))?;

// Fuzzy search
let results = fuzzy_autocomplete_search(&index, "machne", 0.6, Some(5))?;
```

#### GPUI Input with Events
```rust
let input_state = cx.new(|cx| InputState::new(window, cx));

cx.subscribe_in(&input_state, window, |this, _, ev: &InputEvent, window, cx| {
    match ev {
        InputEvent::Change => {
            let value = input_state.read(cx).value();
            // Trigger autocomplete
        }
        _ => {}
    }
});
```

#### CompletionProvider Pattern
```rust
impl CompletionProvider for TerraphimCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        window: &mut Window,
        cx: &mut Context<InputState>,
    ) -> Task<Result<CompletionResponse>> {
        // Get text before cursor
        // Call terraphim_automata search
        // Return CompletionItem array
    }

    fn is_completion_trigger(&self, offset: usize, new_text: &str, cx: &mut Context<InputState>) -> bool {
        new_text == "/" || new_text.chars().all(|c| c.is_alphanumeric())
    }
}
```
