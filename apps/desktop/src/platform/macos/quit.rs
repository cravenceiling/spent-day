use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject},
    sel,
};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};
use std::cell::RefCell;

pub struct Ivars {
    quit: Box<dyn Fn()>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SpentDayQuitTarget"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Target;
    impl Target {
        #[unsafe(method(handleQuit:reply:))]
        fn handle_quit(&self, _event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            (self.ivars().quit)();
        }
    }
);
thread_local! { static TARGET: RefCell<Option<Retained<Target>>> = const { RefCell::new(None) }; }
pub fn install(quit: Box<dyn Fn()>) {
    let mtm = MainThreadMarker::new().expect("quit handler requires main thread");
    let target = Target::alloc(mtm).set_ivars(Ivars { quit });
    let target: Retained<Target> = unsafe { msg_send![super(target), init] };
    unsafe {
        NSAppleEventManager::sharedAppleEventManager()
            .setEventHandler_andSelector_forEventClass_andEventID(
                &target as &AnyObject,
                sel!(handleQuit:reply:),
                u32::from_be_bytes(*b"aevt"),
                u32::from_be_bytes(*b"quit"),
            );
    }
    TARGET.with(|cell| *cell.borrow_mut() = Some(target));
}
