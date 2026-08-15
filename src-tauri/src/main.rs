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
        if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
    }

    aria_lib::run()
}
