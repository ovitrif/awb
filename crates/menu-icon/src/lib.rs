//! Internal rename of the upstream `tray-icon` crate so the rest of the
//! workspace refers to the macOS menu bar status item as `menu-icon` /
//! `MenuBarIcon`. This wrapper is the only place the published crate name
//! appears.

pub use tray_icon::{
    Icon, MouseButton, MouseButtonState, Rect, TrayIcon as MenuBarIcon,
    TrayIconBuilder as MenuBarIconBuilder, TrayIconEvent as MenuBarIconEvent,
};

pub mod menu {
    pub use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
}
