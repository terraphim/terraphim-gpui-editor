# Research Document: Visual Testing Strategy for Terraphim GPUI Editor

## 1. Problem Restatement and Scope

**Problem**: Unit tests for terraphim-gpui-editor fail to compile due to SIGBUS compiler crash on ARM64 Mac when building test harness with GPUI macros. Need alternative testing strategy.

**Solution**: Use visual/story-based testing following Zed/gpui-component patterns.

**IN Scope**:
- Visual verification of autocomplete (Tab/Shift+Tab rotation)
- Visual verification of slash command execution
- Visual verification of full editor integration
- Story crate with interactive examples

**OUT of Scope**:
- Unit tests (blocked by SIGBUS)
- Automated screenshot comparison
- CI integration (manual visual verification only)

## 2. User & Business Outcomes

- Verify Tab cycles forward through completions
- Verify Shift+Tab cycles backward through completions
- Verify slash commands show in menu after typing "/"
- Verify slash command execution replaces input with output
- Provide demo applications for stakeholder review

## 3. System Elements and Dependencies

| Component | Location | Test File |
|-----------|----------|-----------|
| Autocomplete rotation | `gpui-component/input/popovers/completion_menu.rs` | `autocomplete.rs` |
| Slash command menu | `src/completion.rs` | `slash_commands.rs` |
| Full editor | `src/editor.rs` | `editor.rs` |

### Key Behavioral Requirements

**Tab/Shift+Tab in completion menu** (`completion_menu.rs:138-153`):
```rust
.on_action(window.listener_for(&self.list, move |list, action: &IndentInline, window, cx| {
    // Tab cycles forward
    list.update(cx, |list, cx| list.next(window, cx));
}))
.on_action(window.listener_for(&self.list, move |list, action: &OutdentInline, window, cx| {
    // Shift+Tab cycles backward
    list.update(cx, |list, cx| list.prev(window, cx));
}))
```

**Multi-line requirement** (`input.rs:295-296`):
```rust
.when(state.mode.is_multi_line(), |this| {
    this.on_action(window.listener_for(&self.state, InputState::indent_inline))
```
Tab/Shift+Tab handlers ONLY registered for multi-line inputs.

## 4. Constraints and Implications

| Constraint | Implication |
|------------|-------------|
| SIGBUS compiler crash | Cannot use `#[test]` for GPUI code |
| No mocks (CLAUDE.md) | Use real implementations |
| Multi-line required for Tab | All examples must use `.multi_line(true)` |

## 5. Risks, Unknowns, and Assumptions

### Risks
| Risk | Impact | Mitigation |
|------|--------|------------|
| Visual tests not automated | Regressions may slip through | Manual checklist per PR |
| SIGBUS not fixed | Long-term maintenance burden | Monitor rustc updates |

### Assumptions
- **ASSUMED**: Story examples provide sufficient test coverage
- **ASSUMED**: Manual verification is acceptable for this project scale

## 6. Visual Testing Checklist

### Autocomplete Example (`autocomplete.rs`)
- [ ] Type "mach" - completion menu appears with "machine learning"
- [ ] Press Tab - first completion selected and applied
- [ ] Type "ter" - see multiple terraphim-related suggestions
- [ ] Press Tab multiple times - cycles through items
- [ ] Press Shift+Tab - cycles backward
- [ ] Press Escape - menu dismisses
- [ ] Press Ctrl+Space - menu reopens

### Slash Commands Example (`slash_commands.rs`)
- [ ] Type "/" at line start - command menu appears
- [ ] Type "/he" - filtered to "help" and "heading"
- [ ] Select "help" - shows help text
- [ ] Type "/heading 2 My Title" - generates "## My Title"
- [ ] Type "/code rust" - generates code block
- [ ] Type "/list item1, item2" - generates bullet list

### Editor Example (`editor.rs`)
- [ ] Multi-line editing works
- [ ] Both autocomplete and slash commands available
- [ ] Keyboard shortcuts displayed correctly

## 7. Story Crate Structure

```
crates/story/
├── Cargo.toml
├── src/
│   └── main.rs           # Gallery launcher (future)
└── examples/
    ├── autocomplete.rs   # Tab/Shift+Tab testing
    ├── slash_commands.rs # Command execution testing
    └── editor.rs         # Full integration testing
```

## 8. Next Steps

1. Verify all three examples compile and run
2. Execute visual testing checklist
3. Document any failures
4. Proceed to disciplined-design for fixes if needed
