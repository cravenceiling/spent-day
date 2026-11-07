use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject},
    sel,
};
use objc2_app_kit::{NSImage, NSScreen, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::NSString;
use rune::{DisplayTarget, OverlayPlacement};
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

pub struct Ivars {
    on_click: Box<dyn Fn([f64; 4])>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SpentDayStatusTarget"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Target;
    impl Target {
        #[unsafe(method(clicked:))]
        fn clicked(&self, _sender: Option<&AnyObject>) {
            let mtm = MainThreadMarker::from(self);
            STATUS.with(|cell| {
                let binding = cell.borrow();
                let Some((item, _)) = binding.as_ref() else { return };
                let Some(button) = item.button(mtm) else { return };
                let Some(window) = button.window() else { return };
                let frame = window.convertRectToScreen(button.convertRect_toView(button.bounds(), None));
                let height = NSScreen::screens(mtm).firstObject().map_or(0.0, |s| s.frame().size.height);
                (self.ivars().on_click)([frame.origin.x, height - frame.origin.y - frame.size.height, frame.size.width, frame.size.height]);
            });
        }
    }
);
thread_local! { static STATUS: RefCell<Option<(Retained<NSStatusItem>, Retained<Target>)>> = const { RefCell::new(None) }; }
pub fn install(on_click: Box<dyn Fn([f64; 4])>) {
    let mtm = MainThreadMarker::new().expect("status item requires main thread");
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let target = Target::alloc(mtm).set_ivars(Ivars { on_click });
    let target: Retained<Target> = unsafe { msg_send![super(target), init] };
    if let Some(button) = item.button(mtm) {
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("hourglass"),
            Some(&NSString::from_str("Spent Day")),
        ) {
            image.setTemplate(true);
            button.setImage(Some(&image));
        }
        unsafe {
            button.setTarget(Some(&target));
            button.setAction(Some(sel!(clicked:)));
        }
    }
    STATUS.with(|cell| {
        if let Some((old, _)) = cell.borrow_mut().replace((item, target)) {
            NSStatusBar::systemStatusBar().removeStatusItem(&old);
        }
    });
}
pub fn panel_placement(
    [x, button_top, width, height]: [f64; 4],
    size: (f32, f32),
) -> (DisplayTarget, OverlayPlacement) {
    let bottom = button_top + height;
    let mut left = (x + width / 2.0 - f64::from(size.0) / 2.0) as f32;
    let mut top = bottom as f32 + 4.0;
    if let Some(mtm) = MainThreadMarker::new() {
        let screens = NSScreen::screens(mtm);
        let primary_height = screens
            .firstObject()
            .map_or(0.0, |screen| screen.frame().size.height);
        for screen in &screens {
            let frame = screen.frame();
            if x + width / 2.0 >= frame.origin.x
                && x + width / 2.0 < frame.origin.x + frame.size.width
                && bottom >= primary_height - frame.origin.y - frame.size.height
                && bottom <= primary_height - frame.origin.y
            {
                let visible = screen.visibleFrame();
                let min_x = visible.origin.x as f32;
                let min_y = (primary_height - visible.origin.y - visible.size.height) as f32;
                left = left.clamp(
                    min_x,
                    (min_x + visible.size.width as f32 - size.0).max(min_x),
                );
                top = top.clamp(
                    min_y,
                    (min_y + visible.size.height as f32 - size.1).max(min_y),
                );
                break;
            }
        }
    }
    (
        DisplayTarget::Primary,
        OverlayPlacement::Rect {
            x: left,
            y: top,
            width: size.0,
            height: size.1,
        },
    )
}

#[derive(Default)]
pub struct PanelState {
    armed: bool,
    dismissed: Option<Instant>,
}
impl PanelState {
    pub fn focus_changed(&mut self, focused: bool) -> bool {
        if focused {
            self.armed = true;
            false
        } else if self.armed {
            self.armed = false;
            self.dismissed = Some(Instant::now());
            true
        } else {
            false
        }
    }
    pub fn should_open(&mut self, visible: bool) -> bool {
        // Clicking the item can report focus loss before its action reaches the panel.
        let just_dismissed = self
            .dismissed
            .take()
            .is_some_and(|at| at.elapsed() < Duration::from_millis(200));
        self.armed = false;
        !visible && !just_dismissed
    }
}
pub fn panel_options(size: (f32, f32)) -> rune::ConfiguredWindowOptions {
    use objc2_app_kit::NSWindowCollectionBehavior as Behavior;
    let behavior = Behavior::CanJoinAllSpaces
        | Behavior::Stationary
        | Behavior::IgnoresCycle
        | Behavior::FullScreenAuxiliary
        | Behavior::CanJoinAllApplications;
    rune::WindowOptions {
        borderless: true,
        transparent: true,
        resizable: false,
        always_on_top: true,
        visible: false,
        min_size: Some(size),
        max_size: Some(size),
        ..Default::default()
    }
    .macos(
        rune::MacWindowOptions::default()
            .corner_radius(12.0)
            .nonactivating_panel(true)
            .collection_behavior(behavior.bits()),
    )
}
impl PanelState {
    pub fn toggle<S: 'static>(
        &mut self,
        panel: &rune::AppHandle<S>,
        rect: [f64; 4],
        size: (f32, f32),
    ) -> bool {
        if self.should_open(panel.is_visible()) {
            let (display, placement) = panel_placement(rect, size);
            panel.place(display, placement);
            panel.open();
            true
        } else {
            panel.close();
            false
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_arms_only_after_focus_and_click_toggles() {
        let mut state = PanelState::default();
        assert!(!state.focus_changed(false));
        assert!(state.should_open(false));
        assert!(!state.focus_changed(true));
        assert!(state.focus_changed(false));
        assert!(!state.should_open(false));
        assert!(state.should_open(false));
        assert!(!state.should_open(true));
    }
    #[test]
    fn placement_starts_below_the_button_rect() {
        let (target, rect) = panel_placement([200.0, 0.0, 24.0, 24.0], (264.0, 220.0));
        assert_eq!(target, DisplayTarget::Primary);
        assert_eq!(
            rect,
            OverlayPlacement::Rect {
                x: 80.0,
                y: 28.0,
                width: 264.0,
                height: 220.0
            }
        );
    }
}
