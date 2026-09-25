use crate::error::ImagenError;
use crate::types::{ImageBackground, ImageQuality, ImageSize, ImageStyle, OutputFormat};

/// Parse a size string into an ImageSize enum.
/// Accepts standard sizes, "auto", or arbitrary "WxH" per GPT Image's documented
/// constraints: both dimensions divisible by 16, max edge length 3840px, aspect
/// ratio between 1:3 and 3:1, and total pixel count between 655,360 and 8,294,400.
pub fn parse_size(s: &str) -> Result<ImageSize, ImagenError> {
    match s {
        "1024x1024" => Ok(ImageSize::Square),
        "1536x1024" => Ok(ImageSize::Landscape),
        "1024x1536" => Ok(ImageSize::Portrait),
        "auto" => Ok(ImageSize::Auto),
        other => parse_custom_size(other),
    }
}

/// Minimum total pixel count accepted for a custom size, per GPT Image's
/// documented constraints (official OpenAI/Azure docs: 655,360–8,294,400 px).
const MIN_CUSTOM_SIZE_PIXELS: u64 = 655_360;
/// Maximum total pixel count accepted for a custom size (see above).
const MAX_CUSTOM_SIZE_PIXELS: u64 = 8_294_400;

/// Validate and parse an arbitrary WxH size string.
fn parse_custom_size(s: &str) -> Result<ImageSize, ImagenError> {
    let parts: Vec<&str> = s.split('x').collect();
    if parts.len() != 2 {
        return Err(ImagenError::InvalidInput(format!(
            "Invalid size: '{s}'. Use WxH (e.g. 1920x1080), or 1024x1024, 1536x1024, 1024x1536, auto."
        )));
    }

    let width: u32 = parts[0].parse().map_err(|_| {
        ImagenError::InvalidInput(format!("Invalid size: '{s}'. Width must be a number."))
    })?;
    let height: u32 = parts[1].parse().map_err(|_| {
        ImagenError::InvalidInput(format!("Invalid size: '{s}'. Height must be a number."))
    })?;

    if width == 0 || height == 0 {
        return Err(ImagenError::InvalidInput(format!(
            "Invalid size: '{s}'. Width and height must be greater than 0."
        )));
    }
    if !width.is_multiple_of(16) || !height.is_multiple_of(16) {
        return Err(ImagenError::InvalidInput(format!(
            "Invalid size: '{s}'. Both dimensions must be divisible by 16."
        )));
    }
    // Max edge length applies to whichever dimension is longer (e.g. 2160x3840
    // portrait is valid, per official docs, even though height > width here).
    if width.max(height) > 3840 {
        return Err(ImagenError::InvalidInput(format!(
            "Invalid size: '{s}'. Maximum edge length is 3840px."
        )));
    }

    // Aspect ratio check: between 1:3 and 3:1
    let ratio = width as f64 / height as f64;
    if !(1.0 / 3.0..=3.0).contains(&ratio) {
        return Err(ImagenError::InvalidInput(format!(
            "Invalid size: '{s}'. Aspect ratio must be between 1:3 and 3:1."
        )));
    }

    // Total pixel count must fall within gpt-image-2's documented range.
    // Without this check, a size like 512x512 passes every other rule above
    // but is rejected by the provider with a 400 error at request time.
    let pixels = width as u64 * height as u64;
    if pixels < MIN_CUSTOM_SIZE_PIXELS {
        return Err(ImagenError::InvalidInput(format!(
            "Invalid size: '{s}'. Total pixel count ({pixels}) is below the minimum of {MIN_CUSTOM_SIZE_PIXELS} ({} total pixels required, e.g. 1024x640 or larger).",
            MIN_CUSTOM_SIZE_PIXELS
        )));
    }
    if pixels > MAX_CUSTOM_SIZE_PIXELS {
        return Err(ImagenError::InvalidInput(format!(
            "Invalid size: '{s}'. Total pixel count ({pixels}) exceeds the maximum of {MAX_CUSTOM_SIZE_PIXELS}."
        )));
    }

    Ok(ImageSize::Custom(s.to_string()))
}

/// Parse a quality string into an ImageQuality enum.
pub fn parse_quality(s: &str) -> Result<ImageQuality, ImagenError> {
    match s {
        "standard" => Ok(ImageQuality::Standard),
        "hd" => Ok(ImageQuality::Hd),
        "low" => Ok(ImageQuality::Low),
        "medium" => Ok(ImageQuality::Medium),
        "high" => Ok(ImageQuality::High),
        "xhigh" => Ok(ImageQuality::XHigh),
        "max" => Ok(ImageQuality::Max),
        "auto" => Ok(ImageQuality::Auto),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid quality: '{other}'. Use low, medium, high, xhigh, max, auto, standard, or hd."
        ))),
    }
}

/// Parse a style string into an ImageStyle enum.
pub fn parse_style(s: &str) -> Result<ImageStyle, ImagenError> {
    match s {
        "vivid" => Ok(ImageStyle::Vivid),
        "natural" => Ok(ImageStyle::Natural),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid style: '{other}'. Use vivid or natural."
        ))),
    }
}

/// Parse an output format string into an OutputFormat enum.
pub fn parse_format(s: &str) -> Result<OutputFormat, ImagenError> {
    match s {
        "png" => Ok(OutputFormat::Png),
        "webp" => Ok(OutputFormat::Webp),
        "jpeg" | "jpg" => Ok(OutputFormat::Jpeg),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid format: '{other}'. Use png, webp, or jpeg."
        ))),
    }
}

/// Parse a background string into an ImageBackground enum.
pub fn parse_background(s: &str) -> Result<ImageBackground, ImagenError> {
    match s {
        "transparent" => Ok(ImageBackground::Transparent),
        "opaque" => Ok(ImageBackground::Opaque),
        "auto" => Ok(ImageBackground::Auto),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid background: '{other}'. Use transparent, opaque, or auto."
        ))),
    }
}

/// Parse a moderation string. Must be "low" or "auto".
pub fn parse_moderation(s: &str) -> Result<String, ImagenError> {
    match s {
        "low" | "auto" => Ok(s.to_string()),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid moderation: '{other}'. Use low or auto."
        ))),
    }
}

/// Validate an output_compression value (0-100).
pub fn parse_compression(value: u8) -> Result<u8, ImagenError> {
    if value > 100 {
        return Err(ImagenError::InvalidInput(
            "output_compression must be between 0 and 100.".to_string(),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_valid_standard() {
        assert_eq!(parse_size("1024x1024").unwrap(), ImageSize::Square);
        assert_eq!(parse_size("1536x1024").unwrap(), ImageSize::Landscape);
        assert_eq!(parse_size("1024x1536").unwrap(), ImageSize::Portrait);
        assert_eq!(parse_size("auto").unwrap(), ImageSize::Auto);
    }

    #[test]
    fn test_parse_size_custom_valid() {
        assert_eq!(
            parse_size("1920x1088").unwrap(),
            ImageSize::Custom("1920x1088".to_string())
        );
        assert_eq!(
            parse_size("3840x2160").unwrap(),
            ImageSize::Custom("3840x2160".to_string())
        );
        // 2160x3840: portrait 4K, height exceeds the old (buggy) asymmetric
        // height<=2160 cap but is valid per the documented max-edge-length rule.
        assert_eq!(
            parse_size("2160x3840").unwrap(),
            ImageSize::Custom("2160x3840".to_string())
        );
        assert_eq!(
            parse_size("1280x720").unwrap(),
            ImageSize::Custom("1280x720".to_string())
        );
    }

    #[test]
    fn test_parse_size_custom_below_pixel_minimum_rejected() {
        // 512x512 = 262,144 px, below the documented 655,360 minimum.
        let result = parse_size("512x512");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("below the minimum"));
    }

    #[test]
    fn test_parse_size_custom_at_pixel_minimum_accepted() {
        // 1024x640 = 655,360 px, exactly at the documented minimum.
        let result = parse_size("1024x640");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_size_custom_above_pixel_maximum_rejected() {
        // Max edge 3840, but pick a ratio-valid combination that still
        // exceeds the 8,294,400 pixel ceiling while respecting max edge and
        // aspect ratio: 3840x2176 = 8,355,840 px (ratio ~1.77, within 3:1).
        let result = parse_size("3840x2176");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exceeds the maximum"));
    }

    #[test]
    fn test_parse_size_custom_at_pixel_maximum_accepted() {
        // 3840x2160 = 8,294,400 px, exactly at the documented maximum.
        let result = parse_size("3840x2160");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_size_custom_not_divisible_by_16() {
        let result = parse_size("999x999");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("divisible by 16"));
    }

    #[test]
    fn test_parse_size_custom_exceeds_max() {
        let result = parse_size("3856x2160");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Maximum edge length"));
    }

    #[test]
    fn test_parse_size_custom_bad_aspect_ratio() {
        // 3840x640 = 6:1 ratio, exceeds 3:1
        let result = parse_size("3840x640");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Aspect ratio"));
    }

    #[test]
    fn test_parse_size_custom_narrow_valid() {
        // 720x2160 = 1:3 ratio, exactly at the limit
        let result = parse_size("720x2160");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_size_invalid_format() {
        let result = parse_size("notasize");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid size"));
    }

    #[test]
    fn test_parse_quality_valid() {
        assert_eq!(parse_quality("standard").unwrap(), ImageQuality::Standard);
        assert_eq!(parse_quality("hd").unwrap(), ImageQuality::Hd);
        assert_eq!(parse_quality("low").unwrap(), ImageQuality::Low);
        assert_eq!(parse_quality("medium").unwrap(), ImageQuality::Medium);
        assert_eq!(parse_quality("high").unwrap(), ImageQuality::High);
        assert_eq!(parse_quality("xhigh").unwrap(), ImageQuality::XHigh);
        assert_eq!(parse_quality("max").unwrap(), ImageQuality::Max);
        assert_eq!(parse_quality("auto").unwrap(), ImageQuality::Auto);
    }

    #[test]
    fn test_parse_quality_invalid() {
        let result = parse_quality("ultra");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid quality"));
    }

    #[test]
    fn test_parse_style_valid() {
        assert_eq!(parse_style("vivid").unwrap(), ImageStyle::Vivid);
        assert_eq!(parse_style("natural").unwrap(), ImageStyle::Natural);
    }

    #[test]
    fn test_parse_style_invalid() {
        let result = parse_style("dramatic");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid style"));
    }

    #[test]
    fn test_parse_format_valid() {
        assert_eq!(parse_format("png").unwrap(), OutputFormat::Png);
        assert_eq!(parse_format("webp").unwrap(), OutputFormat::Webp);
        assert_eq!(parse_format("jpeg").unwrap(), OutputFormat::Jpeg);
        assert_eq!(parse_format("jpg").unwrap(), OutputFormat::Jpeg);
    }

    #[test]
    fn test_parse_format_invalid() {
        let result = parse_format("bmp");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid format"));
    }

    #[test]
    fn test_parse_background_valid() {
        assert_eq!(
            parse_background("transparent").unwrap(),
            ImageBackground::Transparent
        );
        assert_eq!(parse_background("opaque").unwrap(), ImageBackground::Opaque);
        assert_eq!(parse_background("auto").unwrap(), ImageBackground::Auto);
    }

    #[test]
    fn test_parse_background_invalid() {
        let result = parse_background("gradient");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid background"));
    }

    #[test]
    fn test_parse_moderation_valid() {
        assert_eq!(parse_moderation("low").unwrap(), "low");
        assert_eq!(parse_moderation("auto").unwrap(), "auto");
    }

    #[test]
    fn test_parse_moderation_invalid() {
        let result = parse_moderation("high");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid moderation"));
    }

    #[test]
    fn test_parse_compression_valid() {
        assert_eq!(parse_compression(0).unwrap(), 0);
        assert_eq!(parse_compression(50).unwrap(), 50);
        assert_eq!(parse_compression(100).unwrap(), 100);
    }

    #[test]
    fn test_parse_compression_invalid() {
        // u8 max is 255, so test values above 100
        let result = parse_compression(101);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("output_compression must be between 0 and 100"));
    }
}
