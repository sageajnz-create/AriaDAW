// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK's DMABUF renderer misbehaves on a lot of Linux GPU/driver
    // combinations: the window stops repainting and goes blank grey, typically
    // on focus change or while the app is busy. It is a rendering fault, not a
    // hang — the app is still running underneath.
    //
    // Disabling it costs a little compositing performance and costs us nothing
    // here, since this UI is text and a few controls. Only set when the user
    // hasn't expressed a preference.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }

        // WebKitGTK's Wayland path is the actual culprit for windows that go
        // blank grey — verified that disabling the DMABUF renderer alone did
        // not help while the web process was still running under
        // GDK_BACKEND=wayland. Routing through XWayland is the reliable path.
        //
        // Only override when the session really is Wayland and the user hasn't
        // chosen a backend themselves.
        let wayland = std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
            || std::env::var_os("WAYLAND_DISPLAY").is_some();
        if wayland && std::env::var_os("ARIA_KEEP_WAYLAND").is_none() {
            std::env::set_var("GDK_BACKEND", "x11");
        }

        // Accelerated compositing is what drops the backing surface when the
        // window is hidden and fails to restore it — the window comes back
        // grey after you switch away. Aria's UI is text and a few controls, so
        // there is nothing to gain from compositing it on the GPU, and the GPU
        // is busy generating music anyway.
        if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
    }

    aria_lib::run()
}
