use super::{Collector, Window, run_json};
use serde_json::Value;

pub struct Hyprland;

impl Collector for Hyprland {
    fn name(&self) -> &'static str {
        "hyprland"
    }

    fn active(&mut self) -> Result<Option<Window>, String> {
        run_json("hyprctl", &["activewindow", "-j"]).map(|w| parse(&w))
    }
}

/// `hyprctl activewindow -j` prints `{}` when nothing has focus. `class` is the Wayland app id.
fn parse(window: &Value) -> Option<Window> {
    Window::new(window["class"].as_str().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn reads_the_class() {
        assert_eq!(parse(&fixture("hyprctl-activewindow.json")), Some(Window { app_id: "firefox".into() }));
    }

    #[test]
    fn nothing_focused() {
        assert_eq!(parse(&serde_json::json!({})), None);
    }
}
