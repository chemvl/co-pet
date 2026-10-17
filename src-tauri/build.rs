fn main() {
    // `backend::config()` reads these with `option_env!`.
    println!("cargo:rerun-if-env-changed=COPET_SUPABASE_URL");
    println!("cargo:rerun-if-env-changed=COPET_SUPABASE_KEY");
    // `updater::FEED` (`scripts/release.ts --prefix copet-test`).
    println!("cargo:rerun-if-env-changed=COPET_UPDATE_FEED");
    // `screencapturekit`'s Swift bridge loads `@rpath/libswift_Concurrency.dylib`; a dependency's
    // `rustc-link-arg` never reaches our binaries, so the OS Swift runtime rpath is added here.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    // whisper.cpp's ggml-metal uses `@available`, which calls `__isPlatformVersionAtLeast` from
    // clang's compiler runtime; rustc links with `-nodefaultlibs`, so that archive is added here.
    let resource_dir = std::process::Command::new("cc")
        .arg("-print-resource-dir")
        .output()
        .expect("run `cc -print-resource-dir`");
    let resource_dir = String::from_utf8(resource_dir.stdout).expect("utf-8 clang resource dir");
    println!("cargo:rustc-link-search=native={}/lib/darwin", resource_dir.trim());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
    tauri_build::build()
}
