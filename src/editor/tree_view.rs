//! Shared keyboard-operable outline controls for task and concept trees.

use super::{FocusNextControl, FocusPreviousControl, TerraphimEditor};
use crate::dependency_tree::{TreeGroup, TreeNode};
use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::Sizable;
use std::collections::HashSet;

pub(super) fn render(
    selector: &str,
    groups: &[TreeGroup],
    expanded: &HashSet<String>,
    toggle: impl Fn(&mut TerraphimEditor, String, &mut Context<TerraphimEditor>) + Clone + 'static,
    open: impl Fn(&mut TerraphimEditor, &str, &mut Context<TerraphimEditor>) + Clone + 'static,
    cx: &Context<TerraphimEditor>,
) -> impl IntoElement {
    let mut view = div()
        .id(SharedString::from(selector.to_owned()))
        .flex()
        .flex_col()
        .gap_1()
        .max_h(px(215.))
        .overflow_y_scroll();
    for (index, group) in groups.iter().enumerate() {
        view = view.child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(group.label),
        );
        let path = format!("{selector}-{index}");
        let rows = Rows {
            expanded,
            toggle: &toggle,
            open: &open,
            cx,
        };
        let mut elements = Vec::new();
        rows.append(&mut elements, &group.nodes, &path, 0.);
        view = view.children(elements).when(group.truncated, |view| {
            view.child(div().text_xs().child("More relationships beyond this view"))
        });
    }
    view
}

struct Rows<'a, Toggle, Open> {
    expanded: &'a HashSet<String>,
    toggle: &'a Toggle,
    open: &'a Open,
    cx: &'a Context<'a, TerraphimEditor>,
}

impl<Toggle, Open> Rows<'_, Toggle, Open>
where
    Toggle: Fn(&mut TerraphimEditor, String, &mut Context<TerraphimEditor>) + Clone + 'static,
    Open: Fn(&mut TerraphimEditor, &str, &mut Context<TerraphimEditor>) + Clone + 'static,
{
    fn append(
        &self,
        elements: &mut Vec<gpui::AnyElement>,
        nodes: &[TreeNode],
        path: &str,
        indent: f32,
    ) {
        for (index, node) in nodes.iter().enumerate() {
            let path = format!("{path}-{index}");
            let open = self.open.clone();
            let id = node.id.clone();
            let is_open = self.expanded.contains(&path);
            let expandable = !node.children.is_empty();
            let annotation = if node.cycle {
                " · Cycle reference"
            } else if node.shared {
                " · Shared"
            } else {
                ""
            };
            let label = format!("{}{annotation}", node.label);
            let open_label = if expandable {
                "Open".to_owned()
            } else {
                label.clone()
            };
            let open_selector = format!("{path}-open");
            let open_button = Button::new(SharedString::from(open_selector.clone()))
                .debug_selector(move || open_selector.clone())
                .label(open_label)
                .small()
                .ghost()
                .on_click(
                    self.cx
                        .listener(move |editor, _, _, cx| open(editor, &id, cx)),
                );
            let mut row = div().flex().items_center().gap_1().pl(px(indent));
            if expandable {
                let toggle_path = path.clone();
                let toggle_selector = format!("{path}-toggle");
                let toggle = self.toggle.clone();
                row = row.child(
                    Button::new(SharedString::from(toggle_selector.clone()))
                        .debug_selector(move || toggle_selector.clone())
                        .label(format!(
                            "{} {label}",
                            if is_open { "Collapse" } else { "Expand" }
                        ))
                        .small()
                        .ghost()
                        .on_click(self.cx.listener(move |editor, _, _, cx| {
                            toggle(editor, toggle_path.clone(), cx);
                        })),
                );
            }
            elements.push(row.child(open_button).into_any_element());
            if is_open {
                self.append(elements, &node.children, &path, indent + 14.);
            }
        }
    }
}

pub(super) fn toggle(expanded: &mut HashSet<String>, path: String) {
    if !expanded.remove(&path) {
        expanded.insert(path);
    }
}

fn focus_next(_: &FocusNextControl, window: &mut gpui::Window, cx: &mut gpui::App) {
    cx.stop_propagation();
    window.focus_next();
}

fn focus_previous(_: &FocusPreviousControl, window: &mut gpui::Window, cx: &mut gpui::App) {
    cx.stop_propagation();
    window.focus_prev();
}

pub(super) fn keyboard_navigation<E: InteractiveElement>(element: E, context: &'static str) -> E {
    element
        .key_context(context)
        .on_action(focus_next)
        .on_action(focus_previous)
}
