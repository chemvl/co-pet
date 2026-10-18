/// Spawns a helper process. macOS reaps orphans at startup (`llm::reap_orphans`), so this is spawn.
pub fn spawn_child(cmd: &mut tokio::process::Command) -> std::io::Result<tokio::process::Child> {
    cmd.spawn()
}
