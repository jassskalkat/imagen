use crate::error::ImagenError;
use crate::types::{ImageQuality, ImageSize, ImageStyle, OutputFormat};

/// Parse a size string into an ImageSize enum.
pub fn parse_size(s: &str) -> Result<ImageSize, ImagenError> {
    match s {
        "1024x1024" => Ok(ImageSize::Square),
        "1536x1024" => Ok(ImageSize::Landscape),
        "1024x1536" => Ok(ImageSize::Portrait),
        "auto" => Ok(ImageSize::Auto),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid size: '{other}'. Use 1024x1024, 1536x1024, 1024x1536, or auto."
        ))),
    }
}

/// Parse a quality string into an ImageQuality enum.
pub fn parse_quality(s: &str) -> Result<ImageQuality, ImagenError> {
    match s {
        "standard" => Ok(ImageQuality::Standard),
        "hd" => Ok(ImageQuality::Hd),
        other => Err(ImagenError::InvalidInput(format!(
            "Invalid quality: '{other}'. Use standard or hd."
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_valid() {
        assert_eq!(parse_size("1024x1024").unwrap(), ImageSize::Square);
        assert_eq!(parse_size("1536x1024").unwrap(), ImageSize::Landscape);
        assert_eq!(parse_size("1024x1536").unwrap(), ImageSize::Portrait);
        assert_eq!(parse_size("auto").unwrap(), ImageSize::Auto);
    }

    #[test]
    fn test_parse_size_invalid() {
        let result = parse_size("500x500");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid size"));
    }

    #[test]
    fn test_parse_quality_valid() {
        assert_eq!(parse_quality("standard").unwrap(), ImageQuality::Standard);
        assert_eq!(parse_quality("hd").unwrap(), ImageQuality::Hd);
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
}
