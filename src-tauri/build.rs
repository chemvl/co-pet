fn main() {
    // `backend::config()` reads these with `option_env!`.
    println!("cargo:rerun-if-env-changed=COPET_SUPABASE_URL");
    println!("cargo:rerun-if-env-changed=COPET_SUPABASE_KEY");
    tauri_build::build()
}
