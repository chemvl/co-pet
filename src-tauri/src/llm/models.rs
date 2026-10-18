//! Registry of the local models Co-pet can download and run.
use std::path::PathBuf;

use crate::paths::Paths;

#[derive(Debug)]
pub struct ModelSpec {
    pub id: &'static str,
    pub url: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub label: &'static str,
}

pub const REGISTRY: [ModelSpec; 2] = [
    ModelSpec {
        id: "qwen3-4b-instruct-2507-q4km",
        url: "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        file: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        bytes: 2_497_281_120,
        sha256: "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597",
        label: "Qwen3 4B (2.5 GB), min RAM 16 GB",
    },
    ModelSpec {
        id: "qwen3-1.7b-q4km",
        url: "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf",
        file: "Qwen3-1.7B-Q4_K_M.gguf",
        bytes: 1_107_409_472,
        sha256: "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897",
        label: "Qwen3 1.7B (1.1 GB)",
    },
];

pub fn find(id: &str) -> Option<&'static ModelSpec> {
    REGISTRY.iter().find(|m| m.id == id)
}

/// 4B on machines with at least 15.5 GiB of RAM (a "16 GB" Mac reports slightly less), else 1.7B.
pub fn recommended_model() -> &'static ModelSpec {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let min = (15.5 * 1024.0 * 1024.0 * 1024.0) as u64;
    if sys.total_memory() >= min {
        &REGISTRY[0]
    } else {
        &REGISTRY[1]
    }
}

/// Final, verified model file. Only exists after a successful checksum.
pub fn model_path(paths: &Paths, spec: &ModelSpec) -> PathBuf {
    paths.models().join(spec.file)
}

/// In-progress download.
pub fn part_path(paths: &Paths, spec: &ModelSpec) -> PathBuf {
    paths.models().join(format!("{}.part", spec.file))
}
