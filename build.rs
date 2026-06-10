// build.rs — stage the embedded web UI's wasm into OUT_DIR.
//
// web/unit.wasm is a build artifact, deliberately untracked (CI builds its
// own copy for the Pages deploy; `just wasm` stages one locally). But
// src/features/ws_bridge.rs embeds it at compile time, which makes it a
// compile-time dependency of the binary — a fresh checkout or the crates.io
// package (cargo excludes gitignored files) has no copy. Stage whatever
// exists into OUT_DIR — the real artifact when present, a 0-byte stub when
// not — and let ws_bridge include from OUT_DIR. serve_http recognizes an
// empty embed and answers /unit.wasm with a clear 404 instead of garbage.
//
// OUT_DIR (not the source tree) is load-bearing: `cargo publish` rejects a
// package whose build script modifies the source directory, and read-only
// source builds (distro packaging, sandboxed CI) would fail outright.

use std::path::Path;

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let dest = Path::new(&out_dir).join("unit.wasm");
    // Re-run when the artifact appears or changes (e.g. `just wasm`).
    println!("cargo:rerun-if-changed=web/unit.wasm");

    let src = Path::new("web/unit.wasm");
    if src.exists() {
        std::fs::copy(src, &dest).expect("copy web/unit.wasm into OUT_DIR");
    } else {
        std::fs::write(&dest, []).expect("write unit.wasm stub into OUT_DIR");
    }

    let is_stub = std::fs::metadata(&dest).map(|m| m.len() == 0).unwrap_or(true);
    if is_stub {
        println!(
            "cargo:warning=web/unit.wasm not present — the embedded web UI is a \
             stub in this build (/unit.wasm will return 404). Run `just wasm` \
             and rebuild to bundle the real one."
        );
    }
}
