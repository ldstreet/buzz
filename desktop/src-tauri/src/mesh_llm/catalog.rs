//! Hardware-aware model catalog for the Share-compute picker.
//!
//! Same diagnose pattern as mesh-console: survey the machine's AI memory,
//! rank mesh-llm's curated `MODEL_CATALOG` by how each model fits, mark what
//! is already in the HuggingFace cache, and recommend a best fit. This
//! replaces guessing into a free-text model field.

use serde::Serialize;

use mesh_llm_client::models::catalog::{parse_size_gb, MODEL_CATALOG};
use mesh_llm_client::network::nostr::auto_model_pack;
use mesh_llm_node::models::{default_huggingface_cache_dir, scan_installed_models};
use mesh_llm_system::hardware;
use mesh_llm_system::vram::format_rated_capacity;

/// How a model sits inside this machine's usable AI memory.
/// Mirrors mesh-llm's private `fit_code_for_size_label` thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFit {
    Comfortable,
    Tight,
    Tradeoff,
    TooLarge,
}

fn fit_code(model_gb: f64, vram_gb: f64) -> ModelFit {
    if model_gb <= vram_gb * 0.6 {
        ModelFit::Comfortable
    } else if model_gb <= vram_gb * 0.9 {
        ModelFit::Tight
    } else if model_gb <= vram_gb * 1.1 {
        ModelFit::Tradeoff
    } else {
        ModelFit::TooLarge
    }
}

fn fit_rank(fit: ModelFit) -> u8 {
    match fit {
        ModelFit::Comfortable => 0,
        ModelFit::Tight => 1,
        ModelFit::Tradeoff => 2,
        ModelFit::TooLarge => 3,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshCatalogEntry {
    /// Catalog name — what the user serves (goes straight into the model field).
    pub name: String,
    /// Display size, e.g. "5.0GB".
    pub size: String,
    pub size_gb: f64,
    pub description: String,
    pub fit: ModelFit,
    pub installed: bool,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshModelCatalog {
    /// e.g. "Apple M3 Max"
    pub gpu_name: Option<String>,
    /// Usable AI memory, display-formatted (e.g. "96 GB").
    pub vram_display: String,
    pub vram_gb: f64,
    /// Best-fit catalog name for this hardware, if any.
    pub recommended: Option<String>,
    /// Ranked: recommended first, then by fit, then larger first within a fit.
    pub entries: Vec<MeshCatalogEntry>,
}

/// Survey hardware and rank the curated catalog for this machine.
/// Draft (speculative-decoding) models are excluded — they are not something
/// a person shares directly.
pub fn model_catalog() -> MeshModelCatalog {
    let survey = hardware::survey();
    let vram_gb = survey.vram_bytes as f64 / 1e9;
    build_catalog(
        survey.gpu_name.clone(),
        survey.vram_bytes,
        vram_gb,
        &installed_names(),
        &incomplete_layer_package_files(),
    )
}

fn installed_names() -> Vec<(String, String)> {
    let cache = default_huggingface_cache_dir();
    scan_installed_models(cache)
        .into_iter()
        .map(|m| {
            let file = m
                .path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or_default()
                .to_string();
            (file, m.model_ref)
        })
        .collect()
}

/// Primary GGUF file names of models whose meshllm layer package on disk is
/// *incomplete*. mesh-llm serves such models from the layer package and will
/// resume a multi-GB download on first serve — so "the GGUF is cached" does
/// not mean "starts instantly". mesh-llm's own `scan_installed_models` marks
/// a package installed if any single layer file exists (no completeness
/// check — upstream gap), so we verify against the package's own
/// `model-package.json` manifest: every listed layer file must exist.
fn incomplete_layer_package_files() -> Vec<String> {
    let cache = default_huggingface_cache_dir();
    let mut incomplete = Vec::new();
    let Ok(repos) = std::fs::read_dir(&cache) else {
        return incomplete;
    };
    for repo in repos.flatten() {
        let name = repo.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("models--meshllm--") && name.ends_with("-layers")) {
            continue;
        }
        let Ok(snapshots) = std::fs::read_dir(repo.path().join("snapshots")) else {
            continue;
        };
        for snapshot in snapshots.flatten() {
            let manifest_path = snapshot.path().join("model-package.json");
            let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let Some(primary_file) = manifest["source_model"]["primary_file"].as_str() else {
                continue;
            };
            let layers = manifest["layers"].as_array().cloned().unwrap_or_default();
            let complete = !layers.is_empty()
                && layers.iter().all(|layer| {
                    layer["path"]
                        .as_str()
                        .map(|p| snapshot.path().join(p).exists())
                        .unwrap_or(false)
                });
            if !complete {
                incomplete.push(primary_file.to_string());
            }
        }
    }
    incomplete
}

fn build_catalog(
    gpu_name: Option<String>,
    vram_bytes: u64,
    vram_gb: f64,
    installed: &[(String, String)],
    incomplete_layer_files: &[String],
) -> MeshModelCatalog {
    let is_installed = |file: &str, name: &str| {
        installed
            .iter()
            .any(|(f, model_ref)| f == file || model_ref.contains(name))
    };
    // "Installed" for recommendation purposes means *ready to serve now*:
    // if the model's layer package exists but is incomplete, first serve
    // resumes a multi-GB download — that's not an instant-start pick.
    let is_ready = |file: &str, name: &str| {
        is_installed(file, name) && !incomplete_layer_files.iter().any(|f| f == file)
    };

    let mut entries: Vec<MeshCatalogEntry> = MODEL_CATALOG
        .iter()
        .filter(|m| !is_draft_only(&m.name))
        .map(|m| {
            let size_gb = parse_size_gb(&m.size);
            MeshCatalogEntry {
                fit: fit_code(size_gb, vram_gb),
                installed: is_installed(&m.file, &m.name),
                recommended: false,
                name: m.name.clone(),
                size: m.size.clone(),
                size_gb,
                description: m.description.clone(),
            }
        })
        .collect();

    // Recommendation: prefer what's already on disk. The largest installed
    // general-purpose model that fits comfortably starts instantly — a far
    // better first-run than upstream's pack, which optimizes for a fresh
    // machine and would trigger a multi-GB download. Coder-specialized
    // models are skipped (a shared chat/agent model should be general).
    // Fall back to upstream `auto_model_pack` when nothing suitable is
    // installed.
    let installed_pick = MODEL_CATALOG
        .iter()
        .filter(|m| !is_draft_only(&m.name))
        .filter(|m| is_ready(&m.file, &m.name))
        .filter(|m| {
            let size_gb = parse_size_gb(&m.size);
            fit_code(size_gb, vram_gb) == ModelFit::Comfortable
                && !m.name.to_ascii_lowercase().contains("coder")
        })
        .max_by(|a, b| parse_size_gb(&a.size).total_cmp(&parse_size_gb(&b.size)))
        .map(|m| m.name.clone());
    let recommended = installed_pick.or_else(|| auto_model_pack(vram_gb).into_iter().next());
    for entry in &mut entries {
        entry.recommended = recommended.as_deref() == Some(entry.name.as_str());
    }

    entries.sort_by(|a, b| {
        b.recommended
            .cmp(&a.recommended)
            .then(fit_rank(a.fit).cmp(&fit_rank(b.fit)))
            .then(b.size_gb.total_cmp(&a.size_gb))
    });

    MeshModelCatalog {
        gpu_name,
        vram_display: format_rated_capacity(vram_bytes),
        vram_gb,
        recommended,
        entries,
    }
}

/// A model that exists in the catalog only as another model's draft
/// (speculative decoding helper) — identified by being referenced in any
/// `draft` field. People share chat models, not drafts.
fn is_draft_only(name: &str) -> bool {
    MODEL_CATALOG
        .iter()
        .any(|m| m.draft.as_deref() == Some(name))
        && !MODEL_CATALOG
            .iter()
            .any(|m| m.name == name && m.draft.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_thresholds_match_mesh_llm() {
        // 10GB model on various machines. Thresholds are 0.6 / 0.9 / 1.1.
        assert_eq!(fit_code(10.0, 20.0), ModelFit::Comfortable);
        assert_eq!(fit_code(10.0, 12.0), ModelFit::Tight);
        assert_eq!(fit_code(10.0, 10.0), ModelFit::Tradeoff);
        assert_eq!(fit_code(10.0, 8.0), ModelFit::TooLarge);
    }

    #[test]
    fn catalog_ranks_recommended_first_then_fit() {
        let catalog = build_catalog(Some("Test GPU".into()), 24_000_000_000, 24.0, &[], &[]);
        assert!(
            !catalog.entries.is_empty(),
            "curated catalog must not be empty"
        );
        // The recommended entry (if present in the catalog) must be first.
        if let Some(recommended) = &catalog.recommended {
            if catalog.entries.iter().any(|e| &e.name == recommended) {
                assert_eq!(&catalog.entries[0].name, recommended);
                assert!(catalog.entries[0].recommended);
            }
        }
        // Fit ranks must be non-decreasing after the recommended head.
        let ranks: Vec<u8> = catalog
            .entries
            .iter()
            .skip_while(|e| e.recommended)
            .map(|e| fit_rank(e.fit))
            .collect();
        assert!(
            ranks.windows(2).all(|w| w[0] <= w[1]),
            "fit ranks out of order: {ranks:?}"
        );
    }

    #[test]
    fn recommendation_prefers_largest_installed_comfortable_general_model() {
        // M4 Max 64GB shape: Qwen3-30B-A3B (17GB) and Qwen2.5-3B cached.
        // The installed 30B MoE must win over upstream's download-something
        // pack, and over the smaller installed model. Coder models never
        // win even when installed and larger.
        let installed = vec![
            (
                "Qwen3-30B-A3B-Q4_K_M.gguf".to_string(),
                "unsloth/Qwen3-30B-A3B-GGUF:Q4_K_M".to_string(),
            ),
            (
                "qwen2.5-3b-instruct-q4_k_m.gguf".to_string(),
                "Qwen/Qwen2.5-3B-Instruct-GGUF:q4_k_m".to_string(),
            ),
            (
                "Qwen2.5-Coder-32B-Instruct-Q4_K_M.gguf".to_string(),
                "Qwen/Qwen2.5-Coder-32B-Instruct-GGUF:q4_k_m".to_string(),
            ),
        ];
        let catalog = build_catalog(None, 64_000_000_000, 64.0, &installed, &[]);
        assert_eq!(
            catalog.recommended.as_deref(),
            Some("Qwen3-30B-A3B-Q4_K_M"),
            "largest installed comfortable non-coder model must be recommended"
        );
        // Nothing installed -> upstream pack fallback still recommends.
        let fresh = build_catalog(None, 64_000_000_000, 64.0, &[], &[]);
        assert!(fresh.recommended.is_some());
    }

    #[test]
    fn recommendation_skips_incomplete_layer_packages() {
        // Regression: Qwen3-30B-A3B's GGUF was cached but its meshllm layer
        // package was 7/48 layers — recommending it meant a surprise ~7GB
        // download on first serve. An incomplete package must lose to a
        // smaller fully-ready model; it stays listed as installed, just not
        // recommended.
        let installed = vec![
            (
                "Qwen3-30B-A3B-Q4_K_M.gguf".to_string(),
                "unsloth/Qwen3-30B-A3B-GGUF:Q4_K_M".to_string(),
            ),
            (
                "Qwen3-8B-Q4_K_M.gguf".to_string(),
                "unsloth/Qwen3-8B-GGUF:Q4_K_M".to_string(),
            ),
        ];
        let incomplete = vec!["Qwen3-30B-A3B-Q4_K_M.gguf".to_string()];
        let catalog = build_catalog(None, 64_000_000_000, 64.0, &installed, &incomplete);
        assert_eq!(
            catalog.recommended.as_deref(),
            Some("Qwen3-8B-Q4_K_M"),
            "incomplete layer package must not be the instant-start pick"
        );
        // Same cache with the 30B package complete -> 30B wins again.
        let catalog = build_catalog(None, 64_000_000_000, 64.0, &installed, &[]);
        assert_eq!(catalog.recommended.as_deref(), Some("Qwen3-30B-A3B-Q4_K_M"));
    }

    #[test]
    fn installed_matches_by_file_or_model_ref() {
        let installed = vec![(
            "Qwen3-8B-Q4_K_M.gguf".to_string(),
            "unsloth/Qwen3-8B-GGUF:Q4_K_M".to_string(),
        )];
        let catalog = build_catalog(None, 96_000_000_000, 96.0, &installed, &[]);
        let qwen8b = catalog.entries.iter().find(|e| e.name == "Qwen3-8B-Q4_K_M");
        if let Some(entry) = qwen8b {
            assert!(entry.installed, "cached file must mark entry installed");
        }
        // A machine with nothing installed marks nothing installed.
        let empty = build_catalog(None, 96_000_000_000, 96.0, &[], &[]);
        assert!(empty.entries.iter().all(|e| !e.installed));
    }
}
