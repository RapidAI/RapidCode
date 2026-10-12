//! 桌面入口。
//!
//! 组件树在 `lib.rs` 里（`zcode_app::App`），这样 `tests/smoke.rs` 能直接复用。
//! 这里只负责「把窗口开起来」：设置标题与初始尺寸，然后交给渲染器。

fn main() {
    #[cfg(feature = "desktop")]
    {
        use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
        dioxus::LaunchBuilder::desktop()
            .with_cfg(Config::new().with_window(
                WindowBuilder::new()
                    .with_title(zcode_app::TITLE)
                    .with_inner_size(LogicalSize::new(1360.0, 860.0))
                    .with_min_inner_size(LogicalSize::new(1024.0, 640.0)),
            ))
            .launch(zcode_app::App);
    }

    #[cfg(all(feature = "web", not(feature = "desktop")))]
    {
        dioxus::launch(zcode_app::App);
    }
}
