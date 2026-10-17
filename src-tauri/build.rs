fn main() {
    // `backend::config()` reads these with `option_env!`.
    println!("cargo:rerun-if-env-changed=COPET_SUPABASE_URL");
    println!("cargo:rerun-if-env-changed=COPET_SUPABASE_KEY");
    // `screencapturekit`'s Swift bridge loads `@rpath/libswift_Concurrency.dylib`; a dependency's
    // `rustc-link-arg` never reaches our binaries, so the OS Swift runtime rpath is added here.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    tauri_build::build()
}
