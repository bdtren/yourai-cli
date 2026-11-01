use super::catalog::{Catalog, Kind, Model};
use super::config::Capability;
use super::hardware::{cpu_only, Hardware};

pub const DEFAULT_CTX: u32 = 4096;
/// On CPU-only machines bigger models technically fit in RAM but are too slow to enjoy.
pub const CPU_MAX_PARAMS_B: f64 = 8.5;

/// Why a model with the default quantization does not fit, or None if it fits.
pub fn model_blocker(hw: &Hardware, budget: f64, m: &Model) -> Option<String> {
    let quant = m.default_quant();
    let ctx = if m.kind.is_chat() { DEFAULT_CTX } else { 0 };
    let need = m.memory_gb(quant, ctx)?;
    if m.kind == Kind::Video && cpu_only(hw) {
        return Some("needs a GPU or Apple Silicon; on the CPU a few seconds of video take hours".into());
    }
    if need > budget {
        return Some(format!(
            "needs about {:.1} GB of {}, you have about {:.1} GB",
            need,
            if cpu_only(hw) { "RAM for models" } else { "GPU memory" },
            budget
        ));
    }
    let disk_need = m.download_gb(quant).unwrap_or(0.0) + 1.0;
    if hw.disk_free_gb > 0.0 && disk_need > hw.disk_free_gb {
        return Some(format!(
            "needs {:.1} GB of free disk, you have {:.1} GB",
            disk_need, hw.disk_free_gb
        ));
    }
    None
}

/// Chat models for a goal. With `vision`, only models that can also see images
/// (and, for omni models, hear audio).
pub fn chat_models<'a>(catalog: &'a Catalog, goal: &str, vision: bool) -> Vec<&'a Model> {
    catalog
        .models
        .iter()
        .filter(|m| {
            if vision {
                matches!(m.kind, Kind::Vision | Kind::Omni)
            } else {
                m.kind == Kind::Text && m.tags.iter().any(|t| t == goal)
            }
        })
        .collect()
}

/// Models for a goal, largest first. The recommended one is the largest that fits
/// (capped for CPU-only machines).
pub fn recommend_model<'a>(
    hw: &Hardware,
    catalog: &'a Catalog,
    goal: &str,
    budget: f64,
    vision: bool,
) -> Option<&'a Model> {
    let mut fits: Vec<&Model> = chat_models(catalog, goal, vision)
        .into_iter()
        .filter(|m| model_blocker(hw, budget, m).is_none())
        .collect();
    fits.sort_by(|a, b| b.params_b.total_cmp(&a.params_b));
    let cpu = cpu_only(hw);
    fits.iter()
        .find(|m| !cpu || m.params_b <= CPU_MAX_PARAMS_B)
        .or_else(|| fits.last())
        .copied()
}

/// The model for a capability: the smallest one on CPU-only machines (they are slow),
/// otherwise the best one that fits in `remaining` (memory left next to the chat model),
/// or the smallest that fits on its own.
pub fn recommend_cap_model<'a>(
    hw: &Hardware,
    catalog: &'a Catalog,
    cap: Capability,
    budget: f64,
    remaining: f64,
) -> Option<&'a Model> {
    let mut fits: Vec<&Model> = catalog
        .models_of(cap.kind())
        .filter(|m| model_blocker(hw, budget, m).is_none())
        .collect();
    fits.sort_by(|a, b| a.params_b.total_cmp(&b.params_b));
    let smallest = fits.first().copied();
    if cpu_only(hw) {
        return smallest;
    }
    let roomy: Vec<&Model> = fits
        .into_iter()
        .filter(|m| m.memory_gb(m.default_quant(), 0).is_some_and(|g| g <= remaining))
        .collect();
    let best = match cap {
        // Larger whisper models are slower to answer; small is the sweet spot.
        Capability::SpeechToText => roomy.iter().rev().find(|m| m.params_b < 0.5).or(roomy.last()),
        _ => roomy.last(),
    };
    best.copied().or(smallest)
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
        let a = recommend_model(&small, &c, "chat", budget_gb(&small), false).unwrap();
        let b = recommend_model(&big, &c, "chat", budget_gb(&big), false).unwrap();
        assert!(a.params_b < b.params_b);
        assert!(a.params_b <= 3.5, "8 GB CPU machine got {}", a.id);
        assert!(b.params_b >= 14.0, "24 GB GPU got {}", b.id);
        assert_eq!(a.kind, Kind::Text);
    }

    #[test]
    fn vision_picks_vision_models() {
        let c = Catalog::bundled();
        for (name, hw) in fixtures::all() {
            let m = recommend_model(&hw, &c, "chat", budget_gb(&hw), true);
            assert!(m.is_none_or(|m| matches!(m.kind, Kind::Vision | Kind::Omni)), "{name}");
        }
        let big = fixtures::nvidia_24gb();
        assert!(recommend_model(&big, &c, "code", budget_gb(&big), true).is_some());
    }

    #[test]
    fn capability_models() {
        let c = Catalog::bundled();
        let cpu = fixtures::cpu_only_8gb();
        let b = budget_gb(&cpu);
        assert!(recommend_cap_model(&cpu, &c, Capability::Video, b, b).is_none());
        let img = recommend_cap_model(&cpu, &c, Capability::Image, b, b).unwrap();
        assert_eq!(img.id, "sd-turbo");
        let big = fixtures::nvidia_24gb();
        let b = budget_gb(&big);
        assert!(recommend_cap_model(&big, &c, Capability::Video, b, b).is_some());
        assert_eq!(
            recommend_cap_model(&big, &c, Capability::Image, b, 14.0).unwrap().id,
            "flux1-schnell"
        );
        assert_eq!(
            recommend_cap_model(&big, &c, Capability::Image, b, 3.0).unwrap().id,
            "sd-turbo",
            "falls back to the lightest model when the chat model leaves little room"
        );
        assert_eq!(
            recommend_cap_model(&big, &c, Capability::SpeechToText, b, b)
                .unwrap()
                .id,
            "whisper-small"
        );
    }

    #[test]
    fn cpu_only_is_capped() {
        let c = Catalog::bundled();
        let mut hw = fixtures::cpu_only_8gb();
        hw.ram_gb = 128.0;
        let m = recommend_model(&hw, &c, "chat", budget_gb(&hw), false).unwrap();
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
