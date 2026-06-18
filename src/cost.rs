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
/// - Standard 1024x1024: ~$0.02 per image
/// - HD 1024x1024: ~$0.04 per image
/// - Larger sizes scale proportionally (1.5x for landscape/portrait)
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
    };

    let size_multiplier = match size {
        ImageSize::Square => 1.0,
        ImageSize::Landscape => 1.5,
        ImageSize::Portrait => 1.5,
        ImageSize::Auto => 1.0,
    };

    let per_image = base_cost * size_multiplier;
    let total = per_image * count as f64;

    CostEstimate {
        provider: provider.to_string(),
        model: model.to_string(),
        size: size.as_str().to_string(),
        quality: match quality {
            ImageQuality::Standard => "standard".to_string(),
            ImageQuality::Hd => "hd".to_string(),
        },
        count,
        estimated_cost_usd: total,
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
        // Auto uses the same multiplier as square (1.0)
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
        // Standard portrait: 0.02 * 1.5 = 0.03
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
        // Standard landscape: 0.02 * 1.5 * 2 = 0.06
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
        // HD square: 0.04 * 1.0 * 4 = 0.16
        assert!((est.estimated_cost_usd - 0.16).abs() < 1e-10);
        assert_eq!(est.count, 4);
    }
}
