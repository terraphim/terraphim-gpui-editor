//! GPUI adapter for the Omarchy host's read-only appearance stream.

use futures::{channel::mpsc, SinkExt as _, StreamExt as _};
use gpui::{px, rgba, App, Hsla, Rgba};
use gpui_component::{highlighter::SyntaxColors, scroll::ScrollbarShow, Theme, ThemeMode};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{BufRead as _, BufReader, Read as _},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::mpsc::{sync_channel, SyncSender},
    thread,
    time::Duration,
};

const FIRST_SNAPSHOT_TIMEOUT: Duration = Duration::from_millis(250);
const RECONNECT_MIN: Duration = Duration::from_millis(200);
const RECONNECT_MAX: Duration = Duration::from_secs(5);
const MAX_FRAME_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize, PartialEq)]
struct HostSnapshot {
    v: u64,
    colors: Option<BTreeMap<String, String>>,
    font: Option<HostFont>,
    text: Option<HostText>,
    motion: Option<HostMotion>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct HostFont {
    family: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct HostText {
    base_size: Option<f32>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct HostMotion {
    reduced: Option<bool>,
}

fn socket_path(override_path: Option<OsString>, runtime_dir: Option<OsString>) -> Option<PathBuf> {
    match override_path {
        Some(value) if value.is_empty() || value == "none" => None,
        Some(value) => Some(value.into()),
        None => {
            let runtime_dir = PathBuf::from(runtime_dir?);
            runtime_dir
                .is_absolute()
                .then(|| runtime_dir.join("odk/odkd.sock"))
        }
    }
}

fn read_host(stream: UnixStream, mut publish: impl FnMut(HostSnapshot) -> bool) -> bool {
    let mut reader = BufReader::new(stream);
    let mut received = false;
    let mut line = String::new();
    loop {
        line.clear();
        if !matches!(reader.by_ref().take(MAX_FRAME_BYTES).read_line(&mut line), Ok(n) if n > 0)
            || !line.ends_with('\n')
        {
            return received;
        }
        match serde_json::from_str::<HostSnapshot>(&line) {
            Ok(snapshot) if snapshot.v == 1 => {
                received = true;
                if !publish(snapshot) {
                    return received;
                }
            }
            Ok(_) => log::debug!("Ignoring an unsupported Omarchy appearance snapshot version"),
            Err(error) => log::debug!("Ignoring an invalid Omarchy appearance snapshot: {error}"),
        }
    }
}

fn follow_socket(
    path: &Path,
    first: SyncSender<HostSnapshot>,
    mut updates: mpsc::Sender<HostSnapshot>,
) {
    let mut first = Some(first);
    let mut delay = RECONNECT_MIN;
    while !updates.is_closed() {
        if let Ok(stream) = UnixStream::connect(path) {
            let received = read_host(stream, |snapshot| {
                let snapshot = match first.take() {
                    Some(first) => match first.send(snapshot) {
                        Ok(()) => return true,
                        Err(error) => error.0,
                    },
                    None => snapshot,
                };
                futures::executor::block_on(updates.send(snapshot)).is_ok()
            });
            if received {
                delay = RECONNECT_MIN;
            }
        }
        if !updates.is_closed() {
            thread::sleep(delay);
            delay = (delay * 2).min(RECONNECT_MAX);
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    mode: ThemeMode,
    background: Hsla,
    panel: Hsla,
    sunken: Hsla,
    foreground: Hsla,
    muted: Hsla,
    accent: Hsla,
    selection: Option<Hsla>,
    red: Hsla,
    yellow: Hsla,
    green: Hsla,
    blue: Hsla,
}

impl Palette {
    fn parse(entries: &BTreeMap<String, String>) -> Option<Self> {
        let lookup = |name: &str| entries.get(name).map(String::as_str);
        let named = |name: &str, alias: &str| decode_color(lookup(name).or_else(|| lookup(alias))?);
        let required = |name: &str| decode_color(lookup(name)?);
        let mode = match lookup("mode")? {
            "light" => ThemeMode::Light,
            "dark" => ThemeMode::Dark,
            _ => return None,
        };
        let base = if mode.is_dark() {
            Hsla::black()
        } else {
            Hsla::white()
        };
        let background = base.blend(named("background", "bg")?);
        let over = |color| background.blend(color);
        let selection = lookup("selection_background")
            .or_else(|| lookup("selection"))
            .map(decode_color);
        Some(Self {
            mode,
            background,
            panel: over(named("lighter_background", "lighter_bg")?),
            sunken: over(named("dark_background", "dark_bg")?),
            foreground: over(named("foreground", "fg")?),
            muted: over(required("muted")?),
            accent: over(required("accent")?),
            selection: match selection {
                Some(color) => Some(over(color?)),
                None => None,
            },
            red: over(required("red")?),
            yellow: over(required("yellow")?),
            green: over(required("green")?),
            blue: over(required("blue")?),
        })
    }

    fn apply(self, theme: &mut Theme) {
        let foreground = readable(self.foreground, self.background, 4.5);
        let panel_text = readable(self.foreground, self.panel, 4.5);
        let border = readable(self.muted, self.background, 3.);
        let muted_foreground =
            readable(readable(self.muted, self.background, 4.5), self.panel, 4.5);
        let accent_text = readable(self.foreground, self.accent, 4.5);
        let selection = self.selection.unwrap_or(self.accent);

        theme.background = self.background;
        theme.foreground = foreground;
        theme.border = border;
        theme.caret = foreground;
        theme.input = border;
        theme.ring = readable(self.accent, self.background, 3.);
        theme.link = readable(self.blue, self.background, 4.5);
        theme.link_hover = theme.link;
        theme.link_active = theme.link;
        theme.selection = selection_with_contrast(selection, self.background, foreground);
        theme.muted = self.panel;
        theme.muted_foreground = muted_foreground;
        theme.accent = self.panel;
        theme.accent_foreground = panel_text;
        theme.primary = self.accent;
        theme.primary_foreground = accent_text;
        theme.primary_hover = self.accent.blend(accent_text.opacity(0.10));
        theme.primary_active = self.accent.blend(accent_text.opacity(0.15));
        theme.secondary = self.panel;
        theme.secondary_foreground = panel_text;
        theme.secondary_hover = self.panel.blend(panel_text.opacity(0.1));
        theme.secondary_active = self.panel.blend(panel_text.opacity(0.15));
        theme.popover = self.panel;
        theme.popover_foreground = panel_text;
        theme.list = self.panel;
        theme.list_even = self.panel;
        theme.list_hover = theme.secondary_hover;
        theme.list_active = theme.secondary_active;
        theme.list_active_border = border;
        theme.sidebar = self.panel;
        theme.sidebar_foreground = panel_text;
        theme.sidebar_border = border;
        theme.sidebar_accent = theme.secondary_hover;
        theme.sidebar_accent_foreground = panel_text;
        theme.sidebar_primary = self.accent;
        theme.sidebar_primary_foreground = accent_text;
        theme.scrollbar = self.sunken;
        theme.scrollbar_thumb = readable(self.muted, self.sunken, 3.);
        theme.scrollbar_thumb_hover = readable(self.accent, self.sunken, 3.);
        self.apply_status_colors(theme);
    }

    fn apply_status_colors(self, theme: &mut Theme) {
        theme.danger = self.red;
        theme.danger_foreground = readable(self.foreground, self.red, 4.5);
        theme.danger_hover = self.red.blend(theme.danger_foreground.opacity(0.1));
        theme.danger_active = theme.danger_hover;
        theme.success = self.green;
        theme.success_foreground = readable(self.foreground, self.green, 4.5);
        theme.success_hover = self.green.blend(theme.success_foreground.opacity(0.1));
        theme.success_active = theme.success_hover;
        theme.warning = self.yellow;
        theme.warning_foreground = readable(self.foreground, self.yellow, 4.5);
        theme.warning_hover = self.yellow.blend(theme.warning_foreground.opacity(0.1));
        theme.warning_active = theme.warning_hover;
        theme.info = self.blue;
        theme.info_foreground = readable(self.foreground, self.blue, 4.5);
        theme.info_hover = self.blue.blend(theme.info_foreground.opacity(0.1));
        theme.info_active = theme.info_hover;

        let syntax = std::sync::Arc::make_mut(&mut theme.highlight_theme);
        syntax.style.editor_background = Some(self.background);
        syntax.style.editor_foreground = Some(theme.colors.foreground);
        syntax.style.editor_line_number = Some(theme.colors.muted_foreground);
        syntax.style.editor_active_line_number = Some(theme.colors.foreground);
        syntax.style.editor_active_line = Some(self.panel);
        keep_syntax_legible(&mut syntax.style.syntax, self.panel);
    }
}

fn keep_syntax_legible(syntax: &mut SyntaxColors, surface: Hsla) {
    // ThemeStyle fields are private. Use the component's theme configuration format
    // to adjust foregrounds while preserving its syntax weights and styles.
    let Ok(mut tokens) = serde_json::to_value(&*syntax) else {
        return;
    };
    if let Some(tokens) = tokens.as_object_mut() {
        for token in tokens.values_mut() {
            if let Some(color) = token.get_mut("color") {
                if let Ok(foreground) = Hsla::deserialize(&*color) {
                    let better = readable(surface.blend(foreground), surface, 4.5);
                    if let Ok(encoded) = serde_json::to_value(better) {
                        *color = encoded;
                    }
                }
            }
        }
    }
    if let Ok(updated) = serde_json::from_value(tokens) {
        *syntax = updated;
    }
}

fn decode_color(value: &str) -> Option<Hsla> {
    let hex = value.trim().strip_prefix('#')?;
    let channels = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(rgba((channels << 8) | 0xff).into()),
        8 => Some(rgba(channels).into()),
        _ => None,
    }
}

fn luminance(color: Hsla) -> f32 {
    let rgb = Rgba::from(color);
    let linear = |channel: f32| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b)
}

fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn readable(color: Hsla, surface: Hsla, minimum: f32) -> Hsla {
    if contrast(color, surface) >= minimum {
        return color;
    }
    let white = Hsla::white();
    let black = Hsla::black();
    if contrast(white, surface) >= contrast(black, surface) {
        white
    } else {
        black
    }
}

fn selection_with_contrast(selection: Hsla, surface: Hsla, foreground: Hsla) -> Hsla {
    [1.0, 0.6, 0.4, 0.2]
        .into_iter()
        .map(|opacity| surface.blend(selection.opacity(opacity)))
        .find(|color| contrast(*color, foreground) >= 4.5)
        .unwrap_or(surface.blend(foreground.opacity(0.15)))
}

struct Appearance {
    fallback: Theme,
    palette: Option<Palette>,
    current: Option<HostSnapshot>,
}

impl Appearance {
    fn apply(&mut self, snapshot: HostSnapshot, cx: &mut App) {
        if self.current.as_ref() == Some(&snapshot) {
            return;
        }
        if let Some(palette) = snapshot.colors.as_ref().and_then(Palette::parse) {
            self.palette = Some(palette);
        }
        let mut theme = self.fallback.clone();
        if let Some(palette) = self.palette {
            theme.mode = palette.mode;
            let config = if theme.mode.is_dark() {
                theme.dark_theme.clone()
            } else {
                theme.light_theme.clone()
            };
            theme.apply_config(&config);
            palette.apply(&mut theme);
            theme.radius = px(2.);
            theme.radius_lg = px(2.);
        }
        if let Some(family) = snapshot
            .font
            .as_ref()
            .and_then(|font| font.family.as_deref())
        {
            let family = family.trim();
            if !family.is_empty() {
                theme.font_family = family.to_owned().into();
                theme.mono_font_family.clone_from(&theme.font_family);
            }
        }
        if let Some(size) = snapshot.text.as_ref().and_then(|text| text.base_size) {
            let scale = size.clamp(9., 20.) / 12.;
            theme.font_size *= scale;
            theme.mono_font_size *= scale;
        }
        let reduced_motion = snapshot
            .motion
            .as_ref()
            .and_then(|motion| motion.reduced)
            .unwrap_or(false);
        if reduced_motion {
            // Keep the scrollbar visible and bypass its fade animation.
            theme.scrollbar_show = ScrollbarShow::Always;
        }
        log::info!(
            "Applied Omarchy appearance (mode={}, font={:?}, text={:?}, reduced_motion={reduced_motion})",
            theme.mode.name(),
            theme.font_family,
            theme.font_size,
        );
        cx.set_global(theme);
        cx.refresh_windows();
        self.current = Some(snapshot);
    }
}

pub(super) fn install(cx: &mut App) {
    let Some(path) = socket_path(
        std::env::var_os("ODKD_SOCKET"),
        std::env::var_os("XDG_RUNTIME_DIR"),
    ) else {
        return;
    };
    let wait_for_first = path.exists();
    let (first_sender, first_snapshot) = sync_channel(0);
    let (sender, mut receiver) = mpsc::channel(1);
    if let Err(error) = thread::Builder::new()
        .name("omarchy-appearance".into())
        .spawn(move || follow_socket(&path, first_sender, sender))
    {
        log::warn!("Could not start Omarchy appearance watcher: {error}");
        return;
    }
    let mut appearance = Appearance {
        fallback: Theme::global(cx).clone(),
        palette: None,
        current: None,
    };
    if wait_for_first {
        if let Ok(snapshot) = first_snapshot.recv_timeout(FIRST_SNAPSHOT_TIMEOUT) {
            appearance.apply(snapshot, cx);
        }
    }
    drop(first_snapshot);
    cx.spawn(async move |cx| {
        while let Some(snapshot) = receiver.next().await {
            if cx.update(|cx| appearance.apply(snapshot, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::{io, io::Write as _, os::unix::net::UnixListener, time::Instant};
    use tempfile::tempdir;

    const DARK_COLORS: &str = r##"{
        "mode":"dark","bg":"#151524","fg":"#eeeeeF",
        "dark_background":"#0b0b14","lighter_background":"#292939","muted":"#9999af",
        "accent":"#afc1ff","red":"#ff9494","yellow":"#ffda99","green":"#a8ebaa","blue":"#a3b5ff"
    }"##;

    #[test]
    fn stock_host_path_can_be_overridden_or_disabled() {
        let path = |override_path: Option<&str>, runtime: Option<&str>| {
            socket_path(override_path.map(Into::into), runtime.map(Into::into))
        };
        assert_eq!(
            path(None, Some("/run/user/1200")).as_deref(),
            Some(Path::new("/run/user/1200/odk/odkd.sock"))
        );
        assert_eq!(
            path(Some("/other.sock"), None).as_deref(),
            Some(Path::new("/other.sock"))
        );
        assert_eq!(path(Some("none"), Some("/run/user/1200")), None);
        assert_eq!(path(Some(""), Some("/run/user/1200")), None);
        assert_eq!(path(None, Some("relative")), None);
    }

    fn wait_for<T>(mut try_step: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(value) = try_step() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for appearance update"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn accept(listener: &UnixListener) -> UnixStream {
        listener.set_nonblocking(true).unwrap();
        wait_for(|| match listener.accept() {
            Ok((stream, _)) => Some(stream),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => None,
            Err(error) => panic!("failed to accept test host connection: {error}"),
        })
    }

    #[test]
    fn follows_changes_and_reconnects_to_a_recreated_unix_socket() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("odkd.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (first_sender, first_snapshot) = sync_channel(0);
        let (sender, mut receiver) = mpsc::channel(1);
        let worker_path = path.clone();
        let worker = thread::spawn(move || follow_socket(&worker_path, first_sender, sender));

        let mut client = accept(&listener);
        client
            .write_all(b"{\"v\":1,\"font\":{\"family\":\"Initial Mono\"}}\n")
            .unwrap();
        let first = first_snapshot.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            first.font.and_then(|font| font.family).as_deref(),
            Some("Initial Mono")
        );

        client
            .write_all(
                b"{\"v\":2}\nnot json\n{\"v\":1,\"motion\":{\"reduced\":true},\"text\":{\"base_size\":18}}\n",
            )
            .unwrap();
        let live = wait_for(|| receiver.try_next().ok().flatten());
        assert_eq!(live.motion.and_then(|motion| motion.reduced), Some(true));
        assert_eq!(live.text.and_then(|text| text.base_size), Some(18.));
        drop(client);
        drop(listener);
        std::fs::remove_file(&path).unwrap();

        let replacement = UnixListener::bind(&path).unwrap();
        let mut client = accept(&replacement);
        client
            .write_all(b"{\"v\":1,\"motion\":{\"reduced\":false}}\n")
            .unwrap();
        let reconnected = wait_for(|| receiver.try_next().ok().flatten());
        assert_eq!(
            reconnected.motion.and_then(|motion| motion.reduced),
            Some(false)
        );
        drop(receiver);
        drop(client);
        worker.join().unwrap();
    }

    #[test]
    fn ignores_a_frame_that_exceeds_the_limit() {
        let (server, mut client) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || read_host(server, |_| panic!("unexpected snapshot")));
        let _ = client.write_all(&vec![b'x'; usize::try_from(MAX_FRAME_BYTES).unwrap() + 1]);
        assert!(!worker.join().unwrap());
    }

    #[gpui::test]
    fn applies_live_colors_font_size_and_scrollbar_motion_to_gpui(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            let fallback = Theme::global(cx).clone();
            let mut appearance = Appearance {
                fallback: fallback.clone(),
                palette: None,
                current: None,
            };
            let wire = format!(
                r#"{{"v":1,"colors":{DARK_COLORS},"font":{{"family":"Live Mono"}},"text":{{"base_size":18}},"motion":{{"reduced":true}}}}"#
            );
            appearance.apply(serde_json::from_str(&wire).unwrap(), cx);
            let dark = Theme::global(cx);
            assert_eq!(dark.mode, ThemeMode::Dark);
            assert_eq!(dark.background, decode_color("#151524").unwrap());
            assert_eq!(dark.font_family.as_ref(), "Live Mono");
            assert_eq!(dark.mono_font_family.as_ref(), "Live Mono");
            assert_eq!(dark.font_size, fallback.font_size * 1.5);
            assert_eq!(dark.scrollbar_show, ScrollbarShow::Always);
            assert!(contrast(dark.foreground, dark.background) >= 4.5);
            assert!(contrast(dark.primary_foreground, dark.primary) >= 4.5);

            let comment = dark.highlight_theme.style.syntax.style("comment").unwrap();
            assert!(contrast(comment.color.unwrap(), dark.muted) >= 4.5);
            let dark_background = dark.background;
            appearance.apply(
                serde_json::from_str(
                    r#"{"v":1,"colors":{"mode":"light","background":"invalid"},"motion":{"reduced":false}}"#,
                )
                .unwrap(),
                cx,
            );
            let restored = Theme::global(cx);
            assert_eq!(restored.background, dark_background);
            assert_eq!(restored.font_family, fallback.font_family);
            assert_eq!(restored.font_size, fallback.font_size);
            assert_eq!(restored.scrollbar_show, fallback.scrollbar_show);

            let light = DARK_COLORS
                .replace("dark\"","light\"")
                .replace("#151524", "#e8e9ff")
                .replace("#eeeeeF", "#21231f");
            appearance.apply(serde_json::from_str(&format!(r#"{{"v":1,"colors":{light}}}"#)).unwrap(), cx);
            let light = Theme::global(cx);
            assert_eq!(light.mode, ThemeMode::Light);
            assert_eq!(light.background, decode_color("#e8e9ff").unwrap());
            assert!(contrast(light.foreground, light.background) >= 4.5);
        });
    }
}
