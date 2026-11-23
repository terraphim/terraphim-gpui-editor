//! macOS menu-bar controls for the editor window.

use anyhow::Result;
use futures::{channel::mpsc, StreamExt};
use gpui::{App, Entity, WindowHandle};
use gpui_component::Root;
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use std::{cell::Cell, rc::Rc};
use terraphim_gpui_editor::{editor::RoleChanged, TerraphimEditor};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
    Icon, TrayIconBuilder,
};

const SHOW: &str = "terraphim.show";
const HIDE: &str = "terraphim.hide";
const QUIT: &str = "terraphim.quit";
const ROLE_PREFIX: &str = "terraphim.role.";

pub(crate) fn install(
    window: WindowHandle<Root>,
    editor: &Entity<TerraphimEditor>,
    cx: &mut App,
) -> Result<()> {
    let main_thread = MainThreadMarker::new()
        .ok_or_else(|| anyhow::anyhow!("menu-bar installation requires the main thread"))?;
    let application = NSApplication::sharedApplication(main_thread);
    let show = MenuItem::with_id(SHOW, "Show Editor", true, None);
    let hide = MenuItem::with_id(HIDE, "Hide Editor", true, None);
    let quit = MenuItem::with_id(QUIT, "Quit Terraphim Editor", true, None);
    let roles = Submenu::new("Role", true);
    let items = editor
        .read(cx)
        .role_names()
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let item = CheckMenuItem::with_id(
                format!("{ROLE_PREFIX}{index}"),
                name.as_ref(),
                true,
                index == 0,
                None,
            );
            roles.append(&item)?;
            Ok(item)
        })
        .collect::<Result<Vec<_>>>()?;
    let menu = Menu::with_items(&[
        &show,
        &hide,
        &roles,
        &PredefinedMenuItem::separator(),
        &quit,
    ])?;
    let tray = TrayIconBuilder::new()
        .with_tooltip("Terraphim Editor")
        .with_icon_templated(icon()?)
        .with_menu_on_left_click(true)
        .with_menu(Box::new(menu))
        .build()?;

    window.update(cx, |_, window, cx| {
        window.on_window_should_close(cx, |_, cx| {
            cx.defer(|cx| cx.hide());
            false
        });
    })?;

    let (sender, mut events) = mpsc::unbounded();
    MenuEvent::set_event_handler(Some(move |event| {
        _ = sender.unbounded_send(event);
    }));

    let active_role = Rc::new(Cell::new(0));
    let role_subscription = cx.subscribe(editor, {
        let active_role = active_role.clone();
        let items = items.clone();
        move |_, role: &RoleChanged, _| {
            active_role.set(role.0);
            check_role(&items, role.0);
        }
    });
    let editor = editor.downgrade();
    cx.spawn(async move |cx| {
        let _tray = tray;
        let _role_subscription = role_subscription;
        while let Some(event) = events.next().await {
            cx.update(|cx| match event.id().as_ref() {
                SHOW => {
                    application.unhideWithoutActivation();
                    for native_window in &application.windows() {
                        if native_window.isMiniaturized() {
                            native_window.deminiaturize(None);
                        }
                    }
                    cx.activate(true);
                    _ = window.update(cx, |_, window, _| window.activate_window());
                }
                HIDE => cx.hide(),
                QUIT => cx.quit(),
                id => {
                    if let Some(role) = id
                        .strip_prefix(ROLE_PREFIX)
                        .and_then(|index| index.parse::<usize>().ok())
                    {
                        _ = window.update(cx, |_, window, cx| {
                            _ = editor
                                .update(cx, |editor, cx| editor.select_role(role, window, cx));
                        });
                        check_role(&items, active_role.get());
                    }
                }
            })?;
        }
        Ok::<_, anyhow::Error>(())
    })
    .detach_and_log_err(cx);

    log::info!("Installed Terraphim menu-bar icon");
    Ok(())
}

fn check_role(items: &[CheckMenuItem], active: usize) {
    for (index, item) in items.iter().enumerate() {
        item.set_checked(index == active);
    }
}

fn icon() -> Result<Icon> {
    const SIZE: u32 = 18;
    let mut pixels = Vec::new();
    for y in 0..SIZE {
        for x in 0..SIZE {
            let opaque = ((2..16).contains(&x) && (3..6).contains(&y))
                || ((7..11).contains(&x) && (6..16).contains(&y));
            pixels.extend([0, 0, 0, if opaque { 255 } else { 0 }]);
        }
    }
    Ok(Icon::from_rgba(pixels, SIZE, SIZE)?)
}
