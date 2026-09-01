fn main() {
    tauri_build::build();
    refuse_placeholder_sidecar();
}

/// `scripts/setup.sh` writes a tiny shell script so `cargo test` can run
/// without compiling acestep.cpp. Tauri will happily copy that script into a
/// .deb. Release is what gets bundled, so refuse to produce one that still
/// contains the stand-in.
fn refuse_placeholder_sidecar() {
    if std::env::var("PROFILE").ok().as_deref() != Some("release") {
        return;
    }

    let manifest =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.is_empty() {
        return;
    }

    let mut name = format!("ace-server-{target}");
    if cfg!(windows) {
        name.push_str(".exe");
    }
    let sidecar = manifest.join("binaries").join(&name);
    let Ok(bytes) = std::fs::read(&sidecar) else {
        return;
    };
    const MARKER: &[u8] = b"aria-dev-placeholder";
    let is_placeholder = bytes.windows(MARKER.len()).any(|w| w == MARKER);
    if is_placeholder {
        panic!(
            "\n\
             The engine sidecar at {} is the setup.sh placeholder, not a real\n\
             ace-server. A release build would ship a package that cannot\n\
             generate music.\n\
             \n\
             For a real Linux package:\n\
               ./scripts/package.sh\n\
             \n\
             To just compile the app against a dummy binary (CI layout check):\n\
               ./scripts/package.sh --layout-check\n",
            sidecar.display()
        );
    }
}
