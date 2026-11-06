/// A local model `snet pull` can fetch, pinned to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogModel {
    /// The name the person types, as Ollama names the same model.
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub bytes: u64,
    /// Lowercase hex SHA-256 of the whole file.
    pub sha256: &'static str,
}

/// The model `snet pull` fetches when no name is given.
pub const DEFAULT_MODEL: &str = "qwen3:4b";

/// Sizes and hashes as the host's model API lists them (`lfs.oid`).
pub const CATALOG: [CatalogModel; 3] = [
    CatalogModel {
        name: "qwen3:1.7b",
        file: "Qwen_Qwen3-1.7B-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Qwen_Qwen3-1.7B-GGUF/resolve/main/Qwen_Qwen3-1.7B-Q4_K_M.gguf",
        bytes: 1_282_439_584,
        sha256: "72c5c3cb38fa32d5256e2fe30d03e7a64c6c79e668ad84057e3bd66e250b24fb",
    },
    CatalogModel {
        name: "qwen3:4b",
        file: "Qwen_Qwen3-4B-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Qwen_Qwen3-4B-GGUF/resolve/main/Qwen_Qwen3-4B-Q4_K_M.gguf",
        bytes: 2_497_280_960,
        sha256: "fbe1d5edd4ce802ae3ae7c7e4ab7d09789d697fdac1fc7929f8df4ca3c41bae3",
    },
    CatalogModel {
        name: "qwen3:8b",
        file: "Qwen_Qwen3-8B-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Qwen_Qwen3-8B-GGUF/resolve/main/Qwen_Qwen3-8B-Q4_K_M.gguf",
        bytes: 5_027_784_224,
        sha256: "54fffa050078e984116639c83dfb64b5aa6d4cd474e018b076777c632bbccccd",
    },
];

pub fn catalog_model(name: &str) -> Option<&'static CatalogModel> {
    CATALOG.iter().find(|model| model.name == name)
}
