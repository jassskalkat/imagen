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
    pub estimated_cost_usd: f64,
}

/// Estimate the cost for a gpt-image-2 generation request.
///
/// Pricing is based on known rates:
/// - Low 1024x1024: ~$0.01 per image
/// - Standard/Medium 1024x1024: ~$0.02 per image
/// - HD/High 1024x1024: ~$0.04 per image
/// - Auto: ~$0.02 per image (same as medium/standard)
/// - Larger sizes scale proportionally (1.5x for landscape/portrait)
/// - Custom sizes scale based on pixel count relative to 1024x1024 baseline
pub fn estimate_cost(
    provider: &str,
    model: &str,
    size: &ImageSize,
    quality: &ImageQuality,
    count: u8,
) -> CostEstimate {
    let base_cost = match quality {
        ImageQuality::Standard => 0.02,
        ImageQuality::Hd => 0.04,
        ImageQuality::Low => 0.01,
        ImageQuality::Medium => 0.02,
        ImageQuality::High => 0.04,
        ImageQuality::Auto => 0.02,
    };

    let size_multiplier = match size {
        ImageSize::Square => 1.0,
        ImageSize::Landscape => 1.5,
        ImageSize::Portrait => 1.5,
        ImageSize::Auto => 1.0,
        ImageSize::Custom(s) => custom_size_multiplier(s),
    };

    let per_image = base_cost * size_multiplier;
    let total = per_image * count as f64;

    let quality_str = match quality {
        ImageQuality::Standard => "standard",
        ImageQuality::Hd => "hd",
        ImageQuality::Low => "low",
        ImageQuality::Medium => "medium",
        ImageQuality::High => "high",
        ImageQuality::Auto => "auto",
    };

    CostEstimate {
        provider: provider.to_string(),
        model: model.to_string(),
        size: size.as_str().to_string(),
        quality: quality_str.to_string(),
        count,
        estimated_cost_usd: total,
    }
}

/// Calculate the size multiplier for a custom WxH string relative to 1024x1024.
fn custom_size_multiplier(size_str: &str) -> f64 {
    let baseline_pixels: f64 = 1024.0 * 1024.0;
    let parts: Vec<&str> = size_str.split('x').collect();
    if parts.len() == 2 {
        if let (Ok(w), Ok(h)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
            let pixels = w * h;
            return (pixels / baseline_pixels).max(1.0);
        }
    }
    1.0
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
        assert!((est.estimated_cost_usd - 0.02).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.04).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.06).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.06).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.12).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.02).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.03).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.06).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.16).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.01).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.02).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.04).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.02).abs() < 1e-10);
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
        let expected = 0.02 * (1920.0 * 1088.0) / (1024.0 * 1024.0);
        assert!((est.estimated_cost_usd - expected).abs() < 1e-10);
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
        assert!((est.estimated_cost_usd - 0.02).abs() < 1e-10);
    }
}
