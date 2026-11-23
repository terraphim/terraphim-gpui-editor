use super::{document::Operation, TerraphimEditor};
use gpui::{
    anchored, deferred, div, point, px, Context, Corner, DismissEvent, Entity, EntityInputHandler,
    Focusable, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels,
    Point, Subscription, Window,
};
use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::{RopeExt, Size};

pub(super) struct DocumentContextMenu {
    menu: Entity<PopupMenu>,
    position: Point<Pixels>,
    _dismiss: Subscription,
}

impl DocumentContextMenu {
    pub(super) fn render(&self) -> impl IntoElement {
        deferred(
            anchored()
                .position(self.position)
                .anchor(Corner::TopLeft)
                .snap_to_window_with_margin(px(8.))
                .child(
                    div()
                        .occlude()
                        .debug_selector(|| "document-context-menu".into())
                        .child(self.menu.clone()),
                ),
        )
        .with_priority(1)
    }
}

impl TerraphimEditor {
    pub(super) fn annotation_items(
        menu: PopupMenu,
        editor: gpui::WeakEntity<Self>,
        has_clipboard: bool,
    ) -> PopupMenu {
        let menu = menu.item(PopupMenuItem::new("Edit alternatives").on_click({
            let editor = editor.clone();
            move |_, window, cx| {
                let editor = editor.clone();
                // PopupMenu returns focus to the document after the handler.
                window.defer(cx, move |window, cx| {
                    let _ = editor.update(cx, |editor, cx| {
                        editor.open_inline_alternatives(window, cx);
                    });
                });
            }
        }));
        let menu = [("Next alternative", true), ("Previous alternative", false)]
            .into_iter()
            .fold(menu, |menu, (label, forward)| {
                let editor = editor.clone();
                menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let _ =
                        editor.update(cx, |editor, cx| editor.cycle_at_cursor(forward, window, cx));
                }))
            });
        let operations = [
            (
                "Add clipboard as document alternative",
                Operation::AddAlternative,
                has_clipboard,
            ),
            (
                "Clear document alternatives",
                Operation::ClearAlternatives,
                true,
            ),
            ("Ghost selection", Operation::Ghost, true),
            ("Revive ghosted text", Operation::Revive, true),
            ("Stash selection in Overflow", Operation::Stash, true),
            ("Insert Overflow at cursor", Operation::Pull, true),
        ];
        let menu = operations
            .into_iter()
            .fold(menu, |menu, (label, operation, enabled)| {
                let editor = editor.clone();
                menu.item(PopupMenuItem::new(label).disabled(!enabled).on_click(
                    move |_, window, cx| {
                        let _ = editor.update(cx, |editor, cx| {
                            editor.document_status = editor
                                .apply_document_operation(operation, window, cx)
                                .err()
                                .map(|error| error.to_string().into());
                            cx.notify();
                        });
                    },
                ))
            });
        menu.separator()
            .item(
                PopupMenuItem::new("Copy clean Markdown").on_click(move |_, _, cx| {
                    let _ = editor.update(cx, |editor, cx| {
                        if let Err(error) = editor.copy_clean_markdown(cx) {
                            editor.document_status = Some(error.to_string().into());
                        }
                        cx.notify();
                    });
                }),
            )
    }

    pub(super) fn open_document_context_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Right {
            return;
        }
        cx.stop_propagation();
        window.prevent_default();

        if self
            .selection(window, cx)
            .is_none_or(|range| range.is_empty())
        {
            self.place_context_menu_cursor(event.position, window, cx);
        }
        self.show_document_context_menu(event.position, window, cx);
    }

    fn place_context_menu_cursor(
        &self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.text(cx);
        let role = &self.roles[self.active_role].0;
        let matches = self
            .role_graphs
            .get(role)
            .map(|graph| graph.analyse(&text).matches);
        let graph_ranges = matches
            .into_iter()
            .flatten()
            .map(|term| term.range.start.utf16..term.range.end.utf16);
        let document_ranges = self
            .document
            .annotations
            .spans
            .iter()
            .map(|span| span.anchor.start..span.anchor.end);
        let mut bounds = self.input_bounds.get();
        bounds.origin += point(
            Size::Medium.input_px() + px(1.),
            Size::Medium.input_py() + px(1.),
        );
        self.input_state.update(cx, |input, cx| {
            // Input's public IME point query cannot resolve every logical line.
            // Use the same layout geometry as the visible alternative marks.
            let mark_offset = graph_ranges.chain(document_ranges).find_map(|range| {
                let start = input.bounds_for_range(range.start..range.start, bounds, window, cx)?;
                let end = input.bounds_for_range(range.end..range.end, bounds, window, cx)?;
                (start.top() <= position.y
                    && position.y < end.bottom()
                    && (position.y >= start.bottom() || position.x >= start.left())
                    && (position.y < end.top() || position.x < end.left()))
                .then_some(range.start)
            });
            let offset =
                mark_offset.or_else(|| input.character_index_for_point(position, window, cx));
            if let Some(byte) =
                offset.and_then(|offset| terraphim_alternatives::utf16_to_byte(&text, offset))
            {
                let cursor = input.text().offset_to_position(byte);
                input.set_cursor_position(cursor, window, cx);
            }
        });
    }

    fn show_document_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_selection = self
            .selection(window, cx)
            .is_some_and(|range| !range.is_empty());
        let has_clipboard = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .is_some();
        let has_text = !self.text(cx).is_empty();
        let focus = self.input_state.read(cx).focus_handle(cx);
        let editor = cx.weak_entity();
        let menu = PopupMenu::build(window, cx, move |menu, _window, _cx| {
            let menu = menu
                .action_context(focus)
                .menu("Undo", Box::new(Undo))
                .menu("Redo", Box::new(Redo))
                .separator()
                .menu_with_enable("Cut", Box::new(Cut), has_selection)
                .menu_with_enable("Copy", Box::new(Copy), has_selection)
                .menu_with_enable("Paste", Box::new(Paste), has_clipboard)
                .menu_with_enable("Select All", Box::new(SelectAll), has_text)
                .separator();
            Self::annotation_items(menu, editor, has_clipboard)
        });
        let dismiss = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, _, cx| {
            this.document_menu = None;
            cx.notify();
        });
        menu.focus_handle(cx).focus(window);
        self.document_menu = Some(DocumentContextMenu {
            menu,
            position,
            _dismiss: dismiss,
        });
        cx.notify();
    }
}
