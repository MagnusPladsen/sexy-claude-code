/// Model pricing and cost calculation for token usage.

/// Pricing per 1M tokens for a given model.
#[derive(Debug, Clone, Copy)]
pub struct ModelPricing {
    /// Cost per 1M input tokens in USD.
    pub input_per_million: f64,
    /// Cost per 1M output tokens in USD.
    pub output_per_million: f64,
}

impl ModelPricing {
    /// Calculate total cost for the given token counts.
    pub fn calculate_cost(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        let input_cost = (input_tokens as f64 / 1_000_000.0) * self.input_per_million;
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_per_million;
        input_cost + output_cost
    }
}

/// Look up pricing for a model name. Falls back to Sonnet pricing for unknown models.
pub fn pricing_for_model(model: &str) -> ModelPricing {
    let name = model.to_lowercase();
    if name.contains("opus") {
        ModelPricing {
            input_per_million: 15.0,
            output_per_million: 75.0,
        }
    } else if name.contains("haiku") {
        ModelPricing {
            input_per_million: 0.80,
            output_per_million: 4.0,
        }
    } else {
        // Sonnet or unknown — default to Sonnet pricing
        ModelPricing {
            input_per_million: 3.0,
            output_per_million: 15.0,
        }
    }
}

/// Extract a short display name from a full model identifier.
/// e.g. "claude-sonnet-4-5-20250929" -> "sonnet 4.5"
pub fn short_model_name(model: &str) -> String {
    let lower = model.to_lowercase();
    if lower.contains("opus") {
        "opus".to_string()
    } else if lower.contains("haiku") {
        "haiku".to_string()
    } else if lower.contains("sonnet") {
        "sonnet".to_string()
    } else {
        // Unknown model — use last segment or truncate
        model
            .rsplit('-')
            .find(|s| !s.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(model)
            .to_string()
    }
}

/// Format a cost value as a compact dollar string.
pub fn format_cost(cost: f64) -> String {
    if cost < 0.005 {
        "$0.00".to_string()
    } else if cost < 10.0 {
        format!("${:.2}", cost)
    } else if cost < 100.0 {
        format!("${:.1}", cost)
    } else {
        format!("${:.0}", cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_for_opus() {
        let p = pricing_for_model("claude-opus-4-6");
        assert!((p.input_per_million - 15.0).abs() < f64::EPSILON);
        assert!((p.output_per_million - 75.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_pricing_for_sonnet() {
        let p = pricing_for_model("claude-sonnet-4-5-20250929");
        assert!((p.input_per_million - 3.0).abs() < f64::EPSILON);
        assert!((p.output_per_million - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_pricing_for_haiku() {
        let p = pricing_for_model("claude-haiku-4-5-20251001");
        assert!((p.input_per_million - 0.80).abs() < f64::EPSILON);
        assert!((p.output_per_million - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_pricing_unknown_defaults_to_sonnet() {
        let p = pricing_for_model("some-future-model");
        assert!((p.input_per_million - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_calculate_cost() {
        let p = pricing_for_model("claude-sonnet-4-5-20250929");
        // 1000 input + 500 output with sonnet pricing
        let cost = p.calculate_cost(1000, 500);
        // (1000/1M)*3 + (500/1M)*15 = 0.003 + 0.0075 = 0.0105
        assert!((cost - 0.0105).abs() < 1e-10);
    }

    #[test]
    fn test_calculate_cost_zero() {
        let p = pricing_for_model("claude-sonnet-4-5-20250929");
        assert!((p.calculate_cost(0, 0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_calculate_cost_large() {
        let p = pricing_for_model("claude-opus-4-6");
        // 100k input + 10k output
        let cost = p.calculate_cost(100_000, 10_000);
        // (100000/1M)*15 + (10000/1M)*75 = 1.5 + 0.75 = 2.25
        assert!((cost - 2.25).abs() < 1e-10);
    }

    #[test]
    fn test_format_cost_zero() {
        assert_eq!(format_cost(0.0), "$0.00");
    }

    #[test]
    fn test_format_cost_small() {
        assert_eq!(format_cost(0.03), "$0.03");
        assert_eq!(format_cost(1.24), "$1.24");
    }

    #[test]
    fn test_format_cost_medium() {
        assert_eq!(format_cost(12.5), "$12.5");
    }

    #[test]
    fn test_format_cost_large() {
        assert_eq!(format_cost(150.0), "$150");
    }

    #[test]
    fn test_short_model_name() {
        assert_eq!(short_model_name("claude-opus-4-6"), "opus");
        assert_eq!(short_model_name("claude-sonnet-4-5-20250929"), "sonnet");
        assert_eq!(short_model_name("claude-haiku-4-5-20251001"), "haiku");
    }
}
