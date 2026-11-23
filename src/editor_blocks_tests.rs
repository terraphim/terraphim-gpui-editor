use gpui::{AppContext, Modifiers, TestAppContext};
use gpui_component::{
    input::{Redo, Undo},
    Root,
};

use super::{create_simple_editor, Document};
use crate::document::Operation;

#[gpui::test]
fn block_edit_commits_and_undo_redo_preserve_alternatives(cx: &mut TestAppContext) {
    const ORIGINAL: &str = "# Title\n\nEdit this paragraph.\n\nKeep a clip.\n";
    const EDITED: &str = "# Title\n\nRevised with more detail.\n\nKeep a clip.\n";
    let mut original = Document::new(ORIGINAL);
    let clip = ORIGINAL.rfind("clip").unwrap();
    Operation::AddAlternative
        .apply(&mut original, clip..clip + 4, Some("pin".into()))
        .unwrap();
    Operation::Cycle(-1)
        .apply(&mut original, clip..clip, None)
        .unwrap();
    assert_eq!(original.body, ORIGINAL);

    cx.update(gpui_component::init);
    let mut editor = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| create_simple_editor("Block history", window, cx));
        editor = Some(view.clone());
        Root::new(view, window, cx)
    });
    let editor = editor.unwrap();

    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor
                .load_markdown(&terraphim_alternatives::write(&original), window, cx)
                .unwrap();
            editor.toggle_blocks(window, cx);
            assert!(editor.show_blocks);
        });
    });
    cx.run_until_parked();
    let edit_button = cx.debug_bounds("edit-block-1").expect("Edit button bounds");
    cx.simulate_click(edit_button.center(), Modifiers::none());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.block_draft.is_some());
            assert_eq!(
                editor.block_input.read(cx).value().trim(),
                "Edit this paragraph."
            );
            editor.block_input.update(cx, |input, cx| {
                input.set_value("Revised with more detail.", window, cx);
            });
            assert_eq!(editor.document, original);
        });
    });
    cx.run_until_parked();
    let apply_button = cx.debug_bounds("apply-block").expect("Apply button bounds");
    cx.simulate_click(apply_button.center(), Modifiers::none());
    let committed = cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            assert!(editor.block_draft.is_none());
            assert_eq!(editor.document.body, EDITED);
            let span = &editor.document.annotations.spans[0];
            assert_eq!(span.anchor.start, EDITED.rfind("clip").unwrap());
            assert_eq!(span.anchor.text, "clip");
            editor.toggle_blocks(window, cx);
            editor.document.clone()
        })
    });

    cx.run_until_parked();
    cx.dispatch_action(Undo);
    cx.update(|_, cx| assert_eq!(editor.read(cx).document, original));

    cx.dispatch_action(Redo);
    cx.update(|_, cx| assert_eq!(editor.read(cx).document, committed));
}

#[gpui::test]
fn enter_and_tab_complete_search_shortcuts(cx: &mut TestAppContext) {
    const DOCUMENT: &str =
        "<!-- terraphim:block-id:01M82E6KJRHGQK3AVQSYQ4A7TS -->\nPass me an eraser. Try a clip.\n";
    let commands_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        commands_dir.path().join("search.md"),
        "---\nname: search\nusage: search <pattern>\nparameters:\n  - name: pattern\n    required: true\n---\n# Search\n",
    )
    .unwrap();

    cx.update(gpui_component::init);
    let mut editor = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut editor = create_simple_editor("Command completion", window, cx);
            editor
                .commands
                .register_markdown_commands(commands_dir.path());
            editor
        });
        editor = Some(view.clone());
        Root::new(view, window, cx)
    });
    let editor = editor.unwrap();

    for (shortcut, key) in ["se", "/se", "/sesearch "]
        .into_iter()
        .flat_map(|shortcut| ["enter", "tab"].map(|key| (shortcut, key)))
    {
        let original = format!("{DOCUMENT}{shortcut}");
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.document = Document::new(&original);
                editor.refresh_input(window, cx);
                editor.focus_offset(original.len(), window, cx);
                editor.focus_document(window, cx);
            });
        });
        cx.run_until_parked();
        cx.simulate_keystrokes(key);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let expected = format!("{DOCUMENT}/search ");
                assert_eq!(
                    editor.document.body, expected,
                    "completion of {shortcut} with {key}"
                );
                let caret = editor.selection(window, cx).unwrap();
                assert_eq!(caret, expected.len()..expected.len());
            });
        });
    }
}
