//! Routes menu bar icon clicks through the status button's own action.
//!
//! `tray-icon` attaches the menu to the status item and catches clicks with an
//! overlay view. On recent macOS the status button gets the click itself, so
//! AppKit opened the attached menu on every click and left-click never reached
//! the popover toggle. Here the menu stays detached: a left-click calls
//! `on_primary`, and a right-click or control-click opens the menu.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSEventMask, NSEventModifierFlags, NSEventType, NSMenu, NSStatusItem,
};

pub struct Ivars {
    status_item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    on_primary: Box<dyn Fn()>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AwbStatusClickTarget"]
    #[ivars = Ivars]
    pub struct StatusClickTarget;

    impl StatusClickTarget {
        #[unsafe(method(statusItemClicked:))]
        fn status_item_clicked(&self, _sender: Option<&AnyObject>) {
            let mtm = MainThreadMarker::from(self);
            let opens_menu = NSApplication::sharedApplication(mtm)
                .currentEvent()
                .is_some_and(|event| {
                    matches!(
                        event.r#type(),
                        NSEventType::RightMouseDown | NSEventType::RightMouseUp
                    ) || event.modifierFlags().contains(NSEventModifierFlags::Control)
                });
            if opens_menu {
                self.open_menu(mtm);
            } else {
                (self.ivars().on_primary)();
            }
        }
    }
);

impl StatusClickTarget {
    /// Takes over clicks on `status_item`. The returned target must stay alive
    /// for as long as the status item, since AppKit holds it weakly.
    pub fn install(
        status_item: Retained<NSStatusItem>,
        menu: Retained<NSMenu>,
        on_primary: impl Fn() + 'static,
    ) -> Option<Retained<Self>> {
        let mtm = MainThreadMarker::new()?;
        let button = status_item.button(mtm)?;
        // Hide tray-icon's overlay so every click reaches the button.
        for view in button.subviews() {
            if view.class().name() == c"TaoTrayTarget" {
                view.setHidden(true);
            }
        }
        status_item.setMenu(None);

        let this = Self::alloc(mtm).set_ivars(Ivars {
            status_item,
            menu,
            on_primary: Box::new(on_primary),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // SAFETY: the target outlives the button's use of it (see above) and
        // the selector matches the method defined on the class.
        unsafe {
            button.setTarget(Some(&this));
            button.setAction(Some(sel!(statusItemClicked:)));
        }
        button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
        Some(this)
    }

    fn open_menu(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        let Some(button) = ivars.status_item.button(mtm) else {
            return;
        };
        // Attach the menu only for this click so AppKit places and highlights
        // it natively; performClick blocks until the menu closes.
        ivars.status_item.setMenu(Some(&ivars.menu));
        // SAFETY: called on the main thread with a nil sender, as AppKit expects.
        unsafe { button.performClick(None) };
        ivars.status_item.setMenu(None);
    }
}
