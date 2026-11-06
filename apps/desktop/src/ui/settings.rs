//! Settings stays in the dashboard so drafts survive closing the overlay.

use crate::platform::{self, Pref};
use crate::sync::{self, LastSync, SyncView, Trigger};
use crate::ui::dash::{Dash, Overlay};
use crate::ui::font;
use jiff::Timestamp;
use rune::{Color, El, TextEdit, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Kit, Loader, Switch, TextInput};
use spentday_core::SyncConfig;
use std::time::{Duration, Instant};

pub struct Prefs {
    pub config: Option<Option<SyncConfig>>,
    pub server: TextEdit,
    pub join: Option<TextEdit>,
    pub join_draft: String,
    pub working: bool,
    pub error: Option<String>,
    pub reveal_code: bool,
    pub copied_until: Option<Instant>,
    pub confirm_off: bool,
    pub sync: SyncView,
    pub autostart: Option<Autostart>,
    pub safari: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Autostart {
    pub on: bool,
    pub note: Option<String>,
}

impl Prefs {
    pub fn new() -> Prefs {
        Prefs {
            config: None,
            server: TextEdit::new(""),
            join: None,
            join_draft: String::new(),
            working: false,
            error: None,
            reveal_code: false,
            copied_until: None,
            confirm_off: false,
            sync: SyncView::default(),
            autostart: platform::autostart().map(|on| Autostart { on, note: None }),
            safari: platform::read_pref(Pref::RecordSafariSites),
        }
    }

    pub fn apply_config(&mut self, config: Option<SyncConfig>) {
        if self.config.as_ref() != Some(&config) {
            self.reveal_code = false;
            self.copied_until = None;
        }
        if let Some(c) = &config {
            self.server.replace_text(&c.server_url);
        }
        self.config = Some(config);
    }

    fn begin_join(&mut self) {
        self.join = Some(TextEdit::new(&self.join_draft));
    }
    fn cancel_join(&mut self) {
        if let Some(edit) = self.join.take() {
            self.join_draft = edit.text.clone();
        }
    }
}

/// Call on first open, including `--settings`, after the handles have been registered.
pub fn open(d: &mut Dash, tx: &mut Tx) {
    d.overlay = Overlay::Settings;
    load(d, tx);
}

/// Loads saved settings without opening the overlay.
pub fn load(d: &mut Dash, _tx: &mut Tx) {
    if d.settings.config.is_some() || d.settings.working {
        return;
    }
    let Some(ui) = d.shared.ui.get() else {
        return;
    };
    d.settings.working = true;
    let shared = d.shared.clone();
    crate::jobs::spawn(
        ui.dash.clone(),
        move || {
            shared
                .store
                .sync_config()
                .map_err(|e| crate::format::error("load sync settings", &e))
        },
        |d, result, _| {
            d.settings.working = false;
            match result {
                Ok(config) => {
                    d.shared.sync.lock().unwrap().configured = config.is_some();
                    d.settings.apply_config(config);
                    sync::broadcast(&d.shared);
                }
                Err(error) => d.settings.error = Some(error),
            }
            true
        },
    );
}

pub fn server_problem(value: &str) -> Option<&'static str> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let Some(rest) = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
    else {
        return Some(
            "Enter the full address, starting with https://. For example, https://sync.example.com.",
        );
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    let valid = if let Some(ipv6) = host.strip_prefix('[') {
        ipv6.split_once(']').is_some_and(|(ip, port)| {
            ip.parse::<std::net::Ipv6Addr>().is_ok()
                && (port.is_empty()
                    || port
                        .strip_prefix(':')
                        .is_some_and(|p| p.parse::<u16>().is_ok()))
        })
    } else {
        let (host, port) = host
            .split_once(':')
            .map_or((host, None), |(h, p)| (h, Some(p)));
        !host.is_empty()
            && !host
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '\\' | '[' | ']'))
            && port.is_none_or(|p| p.parse::<u16>().is_ok())
    };
    (!valid).then_some(
        "That doesn't look like a server address. For example, https://sync.example.com.",
    )
}

fn valid_server(p: &Prefs) -> bool {
    !p.server.text.trim().is_empty() && server_problem(&p.server.text).is_none()
}

enum Action {
    Create,
    Join,
    Save,
    Off,
}

fn act(d: &mut Dash, action: Action) {
    if d.settings.working {
        return;
    }
    if !matches!(action, Action::Off) && !valid_server(&d.settings) {
        return;
    }
    let code = match action {
        Action::Join => match &d.settings.join {
            Some(edit) if !edit.text.is_empty() => edit.text.clone(),
            _ => return,
        },
        Action::Save => match d.settings.config.as_ref().and_then(Option::as_ref) {
            Some(c) if c.server_url != d.settings.server.text => c.sync_code.clone(),
            _ => return,
        },
        _ => String::new(),
    };
    let Some(ui) = d.shared.ui.get() else {
        return;
    };
    let handle = ui.dash.clone();
    d.settings.working = true;
    d.settings.error = None;
    d.settings.confirm_off = false;
    let done = move |result: Result<(), String>| {
        handle.update(move |d, _| {
            d.settings.working = false;
            match result {
                Ok(()) => {
                    d.settings.join = None;
                    d.settings.join_draft.clear();
                }
                Err(error) => d.settings.error = Some(error),
            }
        })
    };
    let server = d.settings.server.text.clone();
    match action {
        Action::Create => sync::create(&d.shared, server, done),
        Action::Join | Action::Save => sync::configure(&d.shared, server, code, done),
        Action::Off => sync::turn_off(&d.shared, done),
    }
}

fn copy(d: &mut Dash, tx: &mut Tx) {
    let Some(c) = d.settings.config.as_ref().and_then(Option::as_ref) else {
        return;
    };
    tx.copy_secret(c.sync_code.clone(), Duration::from_secs(60));
    let until = Instant::now() + Duration::from_millis(1500);
    d.settings.copied_until = Some(until);
    tx.wake_after(Duration::from_millis(1500));
    if let Some(ui) = d.shared.ui.get() {
        let handle = ui.dash.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            handle.update_if(move |d, _| {
                if d.settings.copied_until != Some(until) {
                    return false;
                }
                d.settings.copied_until = None;
                true
            });
        });
    }
}

pub fn two_lines(code: &str) -> String {
    let groups: Vec<_> = code.split('-').collect();
    let half = groups.len().div_ceil(2);
    format!("{}\n{}", groups[..half].join("-"), groups[half..].join("-"))
}

fn relative(at: Timestamp) -> String {
    let seconds = (Timestamp::now().as_second() - at.as_second()).max(0);
    match seconds {
        0..60 => "just now".into(),
        60..120 => "1 minute ago".into(),
        120..3600 => format!("{} minutes ago", seconds / 60),
        3600..7200 => "1 hour ago".into(),
        7200..86400 => format!("{} hours ago", seconds / 3600),
        86400..172800 => "yesterday".into(),
        _ => format!("{} days ago", seconds / 86400),
    }
}

fn last_sync(p: &Prefs, k: &Kit) -> El {
    let (label, color) = if p.sync.running {
        ("Syncing…".into(), k.c.text_muted)
    } else {
        match &p.sync.last {
            None => ("Not yet".into(), k.c.text_muted),
            Some(LastSync::Failed { at, message }) => (
                format!("Failed {}. {message}", relative(*at)),
                k.c.danger.fg,
            ),
            Some(LastSync::Ok { at, report }) => (
                format!(
                    "{}  ·  {} sent, {} received",
                    relative(*at),
                    report.pushed_spans,
                    report.pulled_spans
                ),
                k.c.text_muted,
            ),
        }
    };
    let mut content = col()
        .w_full()
        .gap(6.0)
        .child(text("Last sync").font(font::LABEL).color(k.c.text_base))
        .child(
            row()
                .w_full()
                .gap(6.0)
                .center_y()
                .children(p.sync.running.then(|| Loader::new().size(12.0).build(k)))
                .child(
                    text(label)
                        .key("last-sync")
                        .font(font::BODY)
                        .color(color)
                        .wrap()
                        .grow(1.0),
                ),
        );
    if !p.sync.running
        && let Some(LastSync::Ok { report, .. }) = &p.sync.last
        && report.skipped_batches > 0
    {
        let n = report.skipped_batches;
        let message = if n == 1 {
            "1 batch on the server couldn't be read and was skipped. A device may use a different sync code.".into()
        } else {
            format!(
                "{n} batches on the server couldn't be read and were skipped. A device may use a different sync code."
            )
        };
        content = content.child(
            text(message)
                .key("skipped-warning")
                .font(font::BODY)
                .color(k.c.warning.fg)
                .wrap()
                .w_full(),
        );
    }
    content
}

/// Keeps the skeleton signature and modal frame. The dashboard mounts this at its root.
pub fn view(d: &Dash, k: &Kit) -> El {
    let p = &d.settings;
    let config = p.config.as_ref().and_then(Option::as_ref);
    let problem = server_problem(&p.server.text);
    let mut form = col()
        .w_full()
        .gap(14.0)
        .child(
            row()
                .w_full()
                .center_y()
                .gap(8.0)
                .child(text("Sync").font(font::TITLE).color(k.c.text_base))
                .child(el().grow(1.0))
                .child(
                    text(if config.is_some() { "On" } else { "Off" })
                        .font(font::LABEL)
                        .color(if config.is_some() {
                            k.c.text_accent
                        } else {
                            k.c.text_muted
                        }),
                ),
        )
        .child(
            col()
                .w_full()
                .gap(7.0)
                .child(text("Server").font(font::LABEL).color(k.c.text_base))
                .child(
                    TextInput::new(&p.server)
                        .label("Server")
                        .placeholder("https://sync.example.com")
                        .width(None)
                        .invalid(problem.is_some())
                        .disabled(p.working)
                        .on_edit(|d: &mut Dash| &mut d.settings.server)
                        .on_submit(|d: &mut Dash, _| act(d, Action::Save))
                        .build(k)
                        .key("server"),
                ),
        );
    if let Some(problem) = problem {
        form = form.child(
            text(problem)
                .key("server-problem")
                .font(font::BODY)
                .color(k.c.danger.fg)
                .wrap()
                .w_full(),
        );
    }
    if let Some(config) = config {
        let mut code_row = row()
            .w_full()
            .center_y()
            .gap(8.0)
            .child(text("Sync code").font(font::LABEL).color(k.c.text_base))
            .child(el().grow(1.0));
        if !p.reveal_code {
            code_row = code_row.child(
                text("••••-••••-••••-…")
                    .key("masked-code")
                    .label("Hidden")
                    .font(font::BODY.mono())
                    .color(k.c.text_muted),
            );
        }
        form = form.child(
            code_row
                .child(
                    Button::new(if p.reveal_code { "Hide" } else { "Show" })
                        .on_press(|d: &mut Dash| d.settings.reveal_code = !d.settings.reveal_code)
                        .build(k)
                        .key("reveal-code"),
                )
                .child(
                    Button::new(if p.copied_until.is_some_and(|t| t > Instant::now()) {
                        "Copied"
                    } else {
                        "Copy"
                    })
                    .on_press_tx(copy)
                    .build(k)
                    .key("copy-code"),
                ),
        );
        if p.reveal_code {
            form = form.child(
                text(two_lines(&config.sync_code))
                    .key("revealed-code")
                    .font(font::BODY.mono())
                    .color(k.c.text_base)
                    .selectable(true)
                    .w_full(),
            );
        }
        form = form.child(last_sync(p, k));
        let mut actions = row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                Button::new("Turn Off Sync…")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|d: &mut Dash| d.settings.confirm_off = true)
                    .build(k)
                    .key("turn-off"),
            )
            .child(el().grow(1.0));
        if p.server.text != config.server_url {
            actions = actions.child(
                Button::new("Save Server")
                    .disabled(!valid_server(p) || p.working)
                    .on_press(|d: &mut Dash| act(d, Action::Save))
                    .build(k)
                    .key("save-server"),
            );
        }
        form = form.child(
            actions.child(
                Button::new(if p.sync.running {
                    "Syncing…"
                } else {
                    "Sync Now"
                })
                .disabled(p.sync.running)
                .on_press(|d: &mut Dash| sync::run(&d.shared, Trigger::Manual))
                .build(k)
                .key("sync-now"),
            ),
        );
    } else if let Some(join) = &p.join {
        form = form
            .child(
                col()
                    .w_full()
                    .gap(7.0)
                    .child(text("Sync code").font(font::LABEL).color(k.c.text_base))
                    .child(
                        rune::text_input(join, font::BODY.mono())
                            .label("Sync code")
                            .placeholder("Paste the code from another device", k.c.text_muted)
                            .w_full()
                            .min_h(80.0)
                            .padding(10.0)
                            .radius(7.0)
                            .fill(k.c.bg_base)
                            .border(1.0, k.c.border_weak)
                            .color(k.c.text_base)
                            .caret(k.c.text_base, k.selection())
                            .on_edit(|d: &mut Dash| d.settings.join.as_mut().unwrap())
                            .on_input_key(|d: &mut Dash, event, _| {
                                if event.key == rune::Key::Enter {
                                    act(d, Action::Join);
                                    true
                                } else {
                                    false
                                }
                            })
                            .key("join-code"),
                    ),
            )
            .child(
                row()
                    .w_full()
                    .gap(8.0)
                    .child(el().grow(1.0))
                    .child(
                        Button::new("Cancel")
                            .on_press(|d: &mut Dash| d.settings.cancel_join())
                            .build(k)
                            .key("cancel-join"),
                    )
                    .child(
                        Button::new("Join")
                            .variant(ButtonVariant::Submit)
                            .disabled(!valid_server(p) || join.text.is_empty() || p.working)
                            .on_press(|d: &mut Dash| act(d, Action::Join))
                            .build(k)
                            .key("join")
                            .default_button(),
                    ),
            );
    } else {
        form = form.child(
            row()
                .w_full()
                .gap(8.0)
                .child(el().grow(1.0))
                .child(
                    Button::new("Join with Code…")
                        .disabled(!valid_server(p) || p.working)
                        .on_press(|d: &mut Dash| d.settings.begin_join())
                        .build(k)
                        .key("begin-join"),
                )
                .child(
                    Button::new("Create New Sync Code")
                        .variant(ButtonVariant::Submit)
                        .disabled(!valid_server(p) || p.working)
                        .on_press(|d: &mut Dash| act(d, Action::Create))
                        .build(k)
                        .key("create-code"),
                ),
        );
    }
    if let Some(error) = &p.error {
        form = form.child(
            text(error)
                .key("settings-error")
                .role("alert")
                .font(font::BODY)
                .color(k.c.danger.fg)
                .wrap()
                .w_full(),
        );
    }
    let footer = if config.is_some() {
        "Enter this code on your other devices. Anyone with it can read your history, so keep it private. Spent Day syncs every 5 minutes.".into()
    } else {
        format!(
            "Use one sync code on every device to see them all in one dashboard. Sync sends app names, domains, times and this {}'s name, encrypted with the sync code, so the server can't read them.",
            platform::DEVICE_NOUN
        )
    };
    form = form.child(
        text(footer)
            .font(font::BODY)
            .color(k.c.text_muted)
            .wrap()
            .w_full(),
    );
    if p.autostart.is_some() || p.safari.is_some() {
        form = form
            .child(el().w_full().h(1.0).fill(k.c.border_weak))
            .child(text("General").font(font::TITLE).color(k.c.text_base));
    }
    if let Some(auto) = &p.autostart {
        form = form.child(
            row()
                .w_full()
                .center_y()
                .child(
                    text("Launch at login")
                        .font(font::BODY)
                        .color(k.c.text_base),
                )
                .child(el().grow(1.0))
                .child(
                    Switch::new(auto.on)
                        .label("Launch at login")
                        .hide_label()
                        .on_toggle(|d: &mut Dash| {
                            let Some(auto) = &mut d.settings.autostart else {
                                return;
                            };
                            let on = !auto.on;
                            if platform::autostart() == Some(on) {
                                auto.on = on;
                                return;
                            }
                            match platform::set_autostart(on) {
                                Ok(note) => {
                                    auto.on = on;
                                    auto.note = note;
                                }
                                Err(error) => {
                                    auto.on = platform::autostart().unwrap_or(auto.on);
                                    auto.note = Some(error);
                                }
                            }
                        })
                        .build(k)
                        .key("autostart"),
                ),
        );
        if let Some(note) = &auto.note {
            form = form.child(
                text(note)
                    .font(font::BODY)
                    .color(k.c.danger.fg)
                    .wrap()
                    .w_full(),
            );
        }
    }
    if let Some(on) = p.safari {
        form = form.child(row().w_full().center_y().gap(16.0)
            .child(col().grow(1.0).min_w(0.0).gap(4.0)
                .child(text("Record Safari sites").font(font::BODY).color(k.c.text_base))
                .child(text("Spent Day can't tell Safari Private windows apart from normal ones. With this on, private browsing is recorded.")
                    .font(font::BODY).color(k.c.text_muted).wrap().w_full()))
            .child(Switch::new(on).label("Record Safari sites").hide_label()
                .on_toggle(|d: &mut Dash| { if let Some(on) = &mut d.settings.safari {
                    *on = !*on; platform::write_pref(Pref::RecordSafariSites, *on);
                }}).build(k).key("safari")));
    }
    let card = col()
        .key("settings")
        .role("dialog")
        .label("Settings")
        .w(500.0)
        .radius(14.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_weak)
        .child(
            text("Settings")
                .font(rune::TextStyle::new(22.0, 650.0))
                .color(k.c.text_base)
                .px(22.0)
                .pt(20.0)
                .pb(18.0),
        )
        .child(
            col()
                .key("settings-body")
                .w_full()
                .max_h(420.0)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .child(form.padding(22.0).pt(0.0)),
        )
        .child(el().w_full().h(1.0).fill(k.c.border_weak))
        .child(
            row().w_full().padding(14.0).child(el().grow(1.0)).child(
                Button::new("Done")
                    .on_press(|d: &mut Dash| d.overlay = Overlay::None)
                    .build(k)
                    .key("settings-done"),
            ),
        );
    let mut root = el()
        .key("settings-scrim")
        .w_full()
        .h_full()
        .absolute(0.0, 0.0)
        .center()
        .fill(Color::BLACK.alpha(0.45))
        .modal(true, |d: &mut Dash| d.overlay = Overlay::None)
        .child(card);
    if p.confirm_off {
        root = root.child(el().key("off-scrim").w_full().h_full().absolute(0.0, 0.0).center()
            .fill(Color::BLACK.alpha(0.45)).modal(false, |d: &mut Dash| d.settings.confirm_off = false)
            .child(col().key("confirm-off").role("dialog").label("Turn off sync")
                .w(420.0).padding(24.0).gap(16.0).radius(14.0).fill(k.c.layer_01)
                .child(text(format!("Turn off sync on this {}?", platform::DEVICE_NOUN)).font(font::TITLE).color(k.c.text_base))
                .child(text(format!("History already on this {} stays here. To sync again you'll need the sync code, so copy it first if no other device has it.", platform::DEVICE_NOUN))
                    .font(font::BODY).color(k.c.text_muted).wrap().w_full())
                .child(row().w_full().gap(8.0).child(el().grow(1.0))
                    .child(Button::new("Cancel").on_press(|d: &mut Dash| d.settings.confirm_off = false).build(k).key("cancel-off"))
                    .child(Button::new("Turn Off Sync").variant(ButtonVariant::Danger)
                        .on_press(|d: &mut Dash| act(d, Action::Off)).build(k).key("confirm-turn-off")))));
    }
    root
}

#[cfg(test)]
mod tests;
