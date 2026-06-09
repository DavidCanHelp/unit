// build.rs — guarantee the include_bytes! input for the embedded web UI.
//
// web/unit.wasm is a build artifact, deliberately untracked (CI builds its
// own copy for the Pages deploy; `just wasm` stages one locally). But
// src/features/ws_bridge.rs embeds it at compile time via include_bytes!,
// which makes it a compile-time dependency of the native binary — a fresh
// checkout (`cargo install --git`, crates.io package) would not compile
// without it. When the file is absent, write a 0-byte stub so compilation
// succeeds; serve_http in ws_bridge.rs recognizes the empty embed and
// answers /unit.wasm with a clear 404 instead of serving garbage.

use std::path::Path;

fn main() {
    let wasm = Path::new("web/unit.wasm");
    // Re-run when the artifact changes (e.g. `just wasm` replaces the stub),
    // so the warning below reflects the current build. build.rs only writes
    // the file when it is absent, so this cannot self-trigger.
    println!("cargo:rerun-if-changed=web/unit.wasm");

    if !wasm.exists() {
        std::fs::create_dir_all("web").expect("create web/ directory");
        std::fs::write(wasm, []).expect("write web/unit.wasm stub");
    }

    let is_stub = std::fs::metadata(wasm).map(|m| m.len() == 0).unwrap_or(true);
    if is_stub {
        println!(
            "cargo:warning=web/unit.wasm not present — the embedded web UI is a \
             stub in this build (/unit.wasm will return 404). Run `just wasm` \
             and rebuild to bundle the real one."
        );
    }
}
