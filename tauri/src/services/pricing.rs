/// Centralized model pricing registry for calculating estimated API costs.
/// Returns None if pricing is unknown (instead of assuming $0).
pub struct ModelPricing {
    pub input_per_m: f64,
    pub cached_input_per_m: f64,
    pub output_per_m: f64,
    pub cache_write_per_m: f64,
}

pub fn get_model_pricing(model: &str) -> Option<ModelPricing> {
    let lower = model.to_lowercase();
    if lower.contains("opus") {
        Some(ModelPricing {
            input_per_m: 15.0,
            cached_input_per_m: 1.50,
            output_per_m: 75.0,
            cache_write_per_m: 18.75,
        })
    } else if lower.contains("sonnet") {
        Some(ModelPricing {
            input_per_m: 3.0,
            cached_input_per_m: 0.30,
            output_per_m: 15.0,
            cache_write_per_m: 3.75,
        })
    } else if lower.contains("haiku") {
        Some(ModelPricing {
            input_per_m: 0.80,
            cached_input_per_m: 0.08,
            output_per_m: 4.0,
            cache_write_per_m: 1.00,
        })
    } else if lower.contains("mini") {
        Some(ModelPricing {
            input_per_m: 0.40,
            cached_input_per_m: 0.20,
            output_per_m: 1.60,
            cache_write_per_m: 0.40,
        })
    } else if lower.contains("gpt-5") || lower.contains("gpt-6") || lower.contains("codex") {
        Some(ModelPricing {
            input_per_m: 2.50,
            cached_input_per_m: 1.25,
            output_per_m: 10.0,
            cache_write_per_m: 2.50,
        })
    } else {
        None
    }
}

pub fn estimate_cost(
    model: Option<&str>,
    input: u64,
    output: u64,
    cached_input: u64,
    cache_write: u64,
) -> Option<f64> {
    let model = model?;
    let pricing = get_model_pricing(model)?;

    let uncached_input = input.saturating_sub(cached_input);
    let cost = (uncached_input as f64 / 1_000_000.0) * pricing.input_per_m
        + (cached_input as f64 / 1_000_000.0) * pricing.cached_input_per_m
        + (output as f64 / 1_000_000.0) * pricing.output_per_m
        + (cache_write as f64 / 1_000_000.0) * pricing.cache_write_per_m;

    Some(cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_estimate_sonnet_cost() {
        let cost = estimate_cost(Some("claude-sonnet-4-6"), 1_000_000, 100_000, 500_000, 0);
        assert!(cost.is_some());
        let c = cost.unwrap();
        // 500k uncached ($1.50) + 500k cached ($0.15) + 100k out ($1.50) = $3.15
        assert!((c - 3.15).abs() < 0.001);
    }

    #[test]
    fn should_return_none_for_unknown_model() {
        let cost = estimate_cost(Some("custom-unknown-model"), 1_000_000, 100_000, 0, 0);
        assert!(cost.is_none());
    }
}
