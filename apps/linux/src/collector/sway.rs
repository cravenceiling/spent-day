use super::{Collector, Window, run_json};
use serde_json::Value;

pub struct Sway;

impl Collector for Sway {
    fn name(&self) -> &'static str {
        "sway"
    }

    fn active(&mut self) -> Result<Option<Window>, String> {
        run_json("swaymsg", &["-t", "get_tree"]).map(|tree| focused(&tree))
    }
}

/// The focused node of `swaymsg -t get_tree`. A focused workspace or output means no window.
fn focused(node: &Value) -> Option<Window> {
    if node["focused"].as_bool() == Some(true) {
        if !matches!(node["type"].as_str(), Some("con" | "floating_con")) {
            return None;
        }
        let app_id = node["app_id"]
            .as_str()
            .or_else(|| node["window_properties"]["class"].as_str())
            .unwrap_or_default();
        return Window::new(app_id);
    }
    ["nodes", "floating_nodes"]
        .iter()
        .filter_map(|key| node[key].as_array())
        .flatten()
        .find_map(focused)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn finds_the_focused_wayland_window() {
        assert_eq!(
            focused(&fixture("sway-tree-foot.json")),
            Some(Window { app_id: "foot".into() })
        );
    }

    #[test]
    fn empty_workspace_has_no_window() {
        assert_eq!(focused(&fixture("sway-tree-empty.json")), None);
    }

    #[test]
    fn xwayland_window_uses_its_class() {
        assert_eq!(
            focused(&fixture("sway-tree-xwayland.json")),
            Some(Window { app_id: "XTerm".into() })
        );
    }
}
