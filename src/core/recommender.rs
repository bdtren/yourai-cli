use super::catalog::{Catalog, Model};
use super::hardware::{cpu_only, Hardware};

pub const DEFAULT_CTX: u32 = 4096;
/// On CPU-only machines bigger models technically fit in RAM but are too slow to enjoy.
pub const CPU_MAX_PARAMS_B: f64 = 8.5;

/// Why a model with the default quantization does not fit, or None if it fits.
pub fn model_blocker(hw: &Hardware, budget: f64, m: &Model) -> Option<String> {
    let need = m.memory_gb("Q4_K_M", DEFAULT_CTX)?;
    if need > budget {
        return Some(format!(
            "needs about {:.1} GB of {}, you have about {:.1} GB",
            need,
            if cpu_only(hw) { "RAM for models" } else { "GPU memory" },
            budget
        ));
    }
    let disk_need = m.quants.get("Q4_K_M").map(|q| q.size_gb).unwrap_or(0.0) + 1.0;
    if hw.disk_free_gb > 0.0 && disk_need > hw.disk_free_gb {
        return Some(format!(
            "needs {:.1} GB of free disk, you have {:.1} GB",
            disk_need, hw.disk_free_gb
        ));
    }
    None
}

/// Models for a goal, largest first. The recommended one is the largest that fits
/// (capped for CPU-only machines).
pub fn recommend_model<'a>(hw: &Hardware, catalog: &'a Catalog, goal: &str, budget: f64) -> Option<&'a Model> {
    let mut fits: Vec<&Model> = catalog
        .models
        .iter()
        .filter(|m| m.tags.iter().any(|t| t == goal))
        .filter(|m| model_blocker(hw, budget, m).is_none())
        .collect();
    fits.sort_by(|a, b| b.params_b.total_cmp(&a.params_b));
    let cpu = cpu_only(hw);
    fits.iter()
        .find(|m| !cpu || m.params_b <= CPU_MAX_PARAMS_B)
        .or_else(|| fits.last())
        .copied()
}

/// Recommended quantization for a model: Q5 when there is 25% headroom, Q4 when it fits,
/// otherwise Q3.
pub fn recommend_quant(m: &Model, budget: f64, ctx: u32) -> &'static str {
    let fits = |q: &str, factor: f64| m.memory_gb(q, ctx).is_some_and(|need| need * factor <= budget);
    if fits("Q5_K_M", 1.25) {
        "Q5_K_M"
    } else if fits("Q4_K_M", 1.0) || !m.quants.contains_key("Q3_K_M") {
        "Q4_K_M"
    } else {
        "Q3_K_M"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hardware::{budget_gb, fixtures};

    #[test]
    fn recommends_bigger_models_for_bigger_gpus() {
        let c = Catalog::bundled();
        let small = fixtures::cpu_only_8gb();
        let big = fixtures::nvidia_24gb();
        let a = recommend_model(&small, &c, "chat", budget_gb(&small)).unwrap();
        let b = recommend_model(&big, &c, "chat", budget_gb(&big)).unwrap();
        assert!(a.params_b < b.params_b);
        assert!(a.params_b <= 3.5, "8 GB CPU machine got {}", a.id);
        assert!(b.params_b >= 14.0, "24 GB GPU got {}", b.id);
    }

    #[test]
    fn cpu_only_is_capped() {
        let c = Catalog::bundled();
        let mut hw = fixtures::cpu_only_8gb();
        hw.ram_gb = 128.0;
        let m = recommend_model(&hw, &c, "chat", budget_gb(&hw)).unwrap();
        assert!(m.params_b <= CPU_MAX_PARAMS_B);
    }

    #[test]
    fn quant_choice() {
        let c = Catalog::bundled();
        let m = c.model("qwen2.5-7b").unwrap();
        assert_eq!(recommend_quant(m, 24.0, 4096), "Q5_K_M");
        assert_eq!(recommend_quant(m, 6.0, 4096), "Q4_K_M");
        assert_eq!(recommend_quant(m, 4.5, 4096), "Q3_K_M");
    }
}
