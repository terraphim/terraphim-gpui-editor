//! A subset of `FastGPUI`'s GDP wire protocol on GPUI 0.2's test platform.

use super::{editor_window_options, prepare_editor, AppConfig};
use anyhow::{anyhow, bail, Context as _, Result};
use gpui::{
    point, px, Bounds, ClipboardItem, Entity, Keystroke, Modifiers, MouseButton, Pixels,
    TestAppContext, VisualTestContext,
};
use serde_json::{json, Value};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::{
    fs::PermissionsExt as _,
    net::{UnixListener, UnixStream},
};
use std::path::{Path, PathBuf};
use std::time::Duration;
use terraphim_gpui_editor::{headless_indicators, TerraphimEditor};

const MAX_REQUEST: u64 = 1024 * 1024;
const ELEMENTS: &[&str] = &[
    "apply-block",
    "undo-block",
    "cancel-block",
    "markdown-preview",
    "markdown-preview-pane",
    "terraphim-ids-btn",
    "terraphim-blocks-btn",
    "add-inline-alternative",
    "kg-synonyms-panel",
    "document-context-target",
    "document-context-menu",
    "kg-save-synonyms",
    "kg-cancel-synonyms",
    "edit-block-0",
    "edit-block-1",
    "edit-block-2",
    "edit-block-3",
    "edit-block-4",
    "edit-block-5",
    "edit-block-6",
    "edit-block-7",
    "gitea-card-0",
    "gitea-card-1",
    "gitea-card-2",
    "gitea-details-0",
    "gitea-refresh-0",
    "gitea-repository",
    "gitea-title",
    "gitea-description",
    "submit-gitea-issue",
    "create-gitea-issue-0",
    "create-gitea-issue-1",
    "create-gitea-issue-2",
];

const COMMANDS: &[(&str, &str, &[&str])] = &[
    ("commands", "Each supported command and its fields", &[]),
    ("windows", "The editor window id and size", &[]),
    (
        "press",
        "Press a key or chord at the current focus",
        &["key"],
    ),
    ("input", "Type text at the current focus", &["text"]),
    ("type", "Paste text at the current focus", &["text"]),
    (
        "click",
        "Click the centre of a known rendered element",
        &["id"],
    ),
    ("bounds", "Bounds of a known rendered element", &["id"]),
    (
        "right_click",
        "Right-click at a window position",
        &["x", "y"],
    ),
    ("document", "Visible text editor buffer", &[]),
    ("clipboard", "Current test clipboard text", &[]),
    (
        "gitea_cards",
        "Visible Gitea issue cards and refresh state",
        &[],
    ),
    (
        "indicators",
        "The last frame's alternative indicator geometry",
        &[],
    ),
    (
        "advance",
        "Advance the simulated clock by milliseconds",
        &["ms"],
    ),
    ("quit", "Stop the opt-in driver", &[]),
];

pub(super) fn serve(socket: &Path, config: AppConfig) -> Result<()> {
    let parent = socket
        .parent()
        .filter(|path| path.is_absolute())
        .context("GDP_SOCKET must be absolute and in a private directory")?;
    if std::fs::metadata(parent)?.permissions().mode() & 0o077 != 0 {
        bail!("GDP_SOCKET must be in a private directory (mode 0700)");
    }

    let mut driver = Driver::new(config)?;
    let listener = UnixListener::bind(socket)
        .with_context(|| format!("Could not bind {}", socket.display()))?;
    let _remove_socket = SocketFile(socket.to_path_buf());
    for connection in listener.incoming() {
        match serve_connection(&connection?, &mut driver) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) => log::debug!("GDP client disconnected: {error:#}"),
        }
    }
    Ok(())
}

fn serve_connection(mut connection: &UnixStream, driver: &mut Driver) -> Result<bool> {
    let mut reader = BufReader::new(connection.try_clone()?);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.by_ref().take(MAX_REQUEST).read_line(&mut line)? == 0 {
            return Ok(false);
        }
        let complete = line.ends_with('\n');
        let request = if complete {
            serde_json::from_str::<Value>(&line).context("Invalid GDP JSON request")
        } else {
            Err(anyhow!(
                "GDP requests must be newline-terminated and at most 1 MiB"
            ))
        };
        let quit = request
            .as_ref()
            .is_ok_and(|request| request["cmd"] == "quit");
        let reply = match request.and_then(|request| driver.call(&request)) {
            Ok(result) => json!({"ok": true, "result": result}),
            Err(error) => json!({"ok": false, "error": error.to_string()}),
        };
        writeln!(connection, "{reply}")?;
        if quit || !complete {
            return Ok(quit);
        }
    }
}

struct SocketFile(PathBuf);

impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct Driver {
    view: Entity<TerraphimEditor>,
    window: gpui::AnyWindowHandle,
    cx: VisualTestContext,
}

impl Driver {
    fn new(config: AppConfig) -> Result<Self> {
        let cx = TestAppContext::single();
        let (window, view) = cx.update(|cx| {
            gpui_component::init(cx);
            let options = editor_window_options(&config, cx);
            let prepared = prepare_editor(config, cx);
            let mut view = None;
            let window = cx.open_window(options, |window, cx| {
                let (root, editor) = prepared.open(window, cx);
                view = Some(editor);
                root
            })?;
            Ok::<_, anyhow::Error>((window, view.context("Editor window did not open")?))
        })?;
        let window = window.into();
        let mut driver = Self {
            view,
            window,
            cx: VisualTestContext::from_window(window, &cx),
        };
        driver.settle();
        Ok(driver)
    }

    fn settle(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| {
            window.draw(cx).clear();
        });
    }

    fn key(&mut self, key: &str) -> Result<()> {
        let keystroke = Keystroke::parse(key).map_err(|error| anyhow!("{error}"))?;
        self.cx.dispatch_keystroke(self.window, keystroke);
        Ok(())
    }

    fn target(&mut self, request: &Value) -> Result<Bounds<Pixels>> {
        // GPUI 0.2's debug selector API takes a static string. A small allowlist
        // exposes the prototype's card controls without leaking caller strings.
        let id = request["id"]
            .as_str()
            .or_else(|| request["selector"].as_str())
            .context("An element id is required")?;
        let selector = ELEMENTS
            .iter()
            .copied()
            .find(|selector| *selector == id)
            .with_context(|| format!("Unknown element {id:?}"))?;
        self.cx
            .debug_bounds(selector)
            .with_context(|| format!("Element {id:?} is not rendered"))
    }

    fn right_click(&mut self, request: &Value) -> Result<()> {
        let coordinate = |key: &str| -> Result<Pixels> {
            let value = request[key]
                .as_u64()
                .and_then(|value| u16::try_from(value).ok())
                .context("Supply nonnegative integer window coordinates")?;
            Ok(px(f32::from(value)))
        };
        let position = point(coordinate("x")?, coordinate("y")?);
        self.cx
            .simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
        self.cx
            .simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
        Ok(())
    }

    fn call(&mut self, request: &Value) -> Result<Value> {
        let text_field = |name: &str| {
            request[name]
                .as_str()
                .with_context(|| format!("A {name:?} field is required"))
        };
        match text_field("cmd")? {
            "commands" => Ok(COMMANDS
                .iter()
                .map(|(name, doc, params)| json!({ "name": name, "doc": doc, "params": params }))
                .collect()),
            "windows" => Ok(self.cx.update(|window, _| {
                let size = window.viewport_size();
                json!([{
                    "window": self.window.window_id().as_u64(),
                    "size": [f32::from(size.width), f32::from(size.height)]
                }])
            })),
            "gitea_cards" => Ok(self
                .cx
                .update(|_, cx| self.view.read(cx).headless_gitea_cards())),
            "document" => Ok(self.cx.update(|_, cx| json!(self.view.read(cx).text(cx)))),
            "clipboard" => Ok(self
                .cx
                .update(|_, cx| json!(cx.read_from_clipboard().and_then(|item| item.text())))),
            "indicators" => Ok(self.cx.update(|_, cx| headless_indicators(cx))),
            "bounds" => {
                let bounds = self.target(request)?;
                Ok(json!([
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height)
                ]))
            }
            command => {
                match command {
                    "press" => self.key(text_field("key")?)?,
                    "input" => {
                        for symbol in text_field("text")?.chars() {
                            match symbol {
                                '\n' => self.key("enter")?,
                                '\t' => self.key("tab")?,
                                ' ' => self.key("space")?,
                                _ => self.key(&symbol.to_string())?,
                            }
                        }
                    }
                    "type" => {
                        self.cx.write_to_clipboard(ClipboardItem::new_string(
                            text_field("text")?.to_owned(),
                        ));
                        self.key(if cfg!(target_os = "macos") {
                            "cmd-v"
                        } else {
                            "ctrl-v"
                        })?;
                    }
                    "click" => {
                        let point = self.target(request)?.center();
                        self.cx.simulate_click(point, Modifiers::none());
                    }
                    "right_click" => self.right_click(request)?,
                    "advance" => {
                        let ms = request["ms"]
                            .as_u64()
                            .context("A nonnegative ms value is required")?;
                        if ms > 60_000 {
                            bail!("Advance at most 60000 ms per request");
                        }
                        self.cx
                            .background_executor
                            .advance_clock(Duration::from_millis(ms));
                    }
                    "quit" => return Ok(Value::Null),
                    _ => bail!("Unknown GDP command {command:?}"),
                }
                self.settle();
                Ok(Value::Null)
            }
        }
    }
}
