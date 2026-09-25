use serde::{Deserialize, Serialize};

use crate::types::{ImageQuality, ImageSize};

/// Cost estimate for an image generation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub provider: String,
    pub model: String,
    pub size: String,
    pub quality: String,
    pub count: u8,
    pub estimated_cost_usd: Option<f64>,
    pub note: String,
}

/// Return a cost estimate envelope without inventing a price.
///
/// OpenAI's current GPT Image pricing is token-based and the official
/// documentation directs callers to the image-generation calculator. Exact
/// cost is available after the provider returns usage, so this tool reports
/// `null` until a usage-aware estimate is implemented.
pub fn estimate_cost(
    provider: &str,
    model: &str,
    size: &ImageSize,
    quality: &ImageQuality,
    count: u8,
) -> CostEstimate {
    let quality_str = quality.as_api_str();

    CostEstimate {
        provider: provider.to_string(),
        model: model.to_string(),
        size: size.as_str().to_string(),
        quality: quality_str.to_string(),
        count,
        estimated_cost_usd: None,
        note: "Exact image cost depends on provider usage and current pricing. Use the official image-generation calculator; this runtime does not yet return a post-generation cost.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_square_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Square,
            &ImageQuality::Standard,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
    }

    #[test]
    fn test_hd_square_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Square,
            &ImageQuality::Hd,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
    }

    #[test]
    fn test_hd_landscape_cost() {
        let est = estimate_cost(
            "azure",
            "gpt-image-2",
            &ImageSize::Landscape,
            &ImageQuality::Hd,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
    }

    #[test]
    fn test_multiple_images_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Square,
            &ImageQuality::Standard,
            3,
        );
        assert!(est.estimated_cost_usd.is_none());
    }

    #[test]
    fn test_cost_estimate_fields() {
        let est = estimate_cost(
            "azure",
            "gpt-image-2",
            &ImageSize::Portrait,
            &ImageQuality::Hd,
            2,
        );
        assert_eq!(est.provider, "azure");
        assert_eq!(est.model, "gpt-image-2");
        assert_eq!(est.size, "1024x1536");
        assert_eq!(est.quality, "hd");
        assert_eq!(est.count, 2);
        assert!(est.estimated_cost_usd.is_none());
    }

    #[test]
    fn test_auto_size_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Auto,
            &ImageQuality::Standard,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
        assert_eq!(est.size, "auto");
    }

    #[test]
    fn test_standard_portrait_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Portrait,
            &ImageQuality::Standard,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
    }

    #[test]
    fn test_standard_landscape_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Landscape,
            &ImageQuality::Standard,
            2,
        );
        assert!(est.estimated_cost_usd.is_none());
    }

    #[test]
    fn test_max_images_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Square,
            &ImageQuality::Hd,
            4,
        );
        assert!(est.estimated_cost_usd.is_none());
        assert_eq!(est.count, 4);
    }

    #[test]
    fn test_low_quality_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Square,
            &ImageQuality::Low,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
        assert_eq!(est.quality, "low");
    }

    #[test]
    fn test_medium_quality_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Square,
            &ImageQuality::Medium,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
        assert_eq!(est.quality, "medium");
    }

    #[test]
    fn test_high_quality_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Square,
            &ImageQuality::High,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
        assert_eq!(est.quality, "high");
    }

    #[test]
    fn test_auto_quality_cost() {
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Square,
            &ImageQuality::Auto,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
        assert_eq!(est.quality, "auto");
    }

    #[test]
    fn test_custom_size_cost() {
        // 1920x1088 = 2,088,960 pixels vs 1,048,576 baseline
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Custom("1920x1088".to_string()),
            &ImageQuality::Standard,
            1,
        );
        assert_eq!(est.size, "1920x1088");
        assert!(est.estimated_cost_usd.is_none());
    }

    #[test]
    fn test_custom_size_smaller_than_baseline_uses_minimum() {
        // 512x512 = 262,144 pixels, less than baseline, clamped to 1.0
        let est = estimate_cost(
            "openai",
            "gpt-image-2",
            &ImageSize::Custom("512x512".to_string()),
            &ImageQuality::Standard,
            1,
        );
        assert!(est.estimated_cost_usd.is_none());
    }
}
