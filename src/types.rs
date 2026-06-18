use serde::{Deserialize, Serialize};

/// Supported image sizes for generation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum ImageSize {
    #[serde(rename = "1024x1024")]
    #[default]
    Square,
    #[serde(rename = "1536x1024")]
    Landscape,
    #[serde(rename = "1024x1536")]
    Portrait,
    #[serde(rename = "auto")]
    Auto,
}

impl ImageSize {
    /// Return the size string suitable for the API.
    pub fn as_str(&self) -> &'static str {
        match self {
            ImageSize::Square => "1024x1024",
            ImageSize::Landscape => "1536x1024",
            ImageSize::Portrait => "1024x1536",
            ImageSize::Auto => "auto",
        }
    }
}

/// Image quality level.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ImageQuality {
    #[default]
    Standard,
    Hd,
}

/// Output format for generated images.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Png,
    Webp,
    Jpeg,
}

impl OutputFormat {
    /// Return the file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
            OutputFormat::Jpeg => "jpeg",
        }
    }

    /// Return the MIME type for this format.
    #[allow(dead_code)] // Part of the public API for HTTP transport (future SSE/HTTP mode)
    pub fn mime_type(&self) -> &'static str {
        match self {
            OutputFormat::Png => "image/png",
            OutputFormat::Webp => "image/webp",
            OutputFormat::Jpeg => "image/jpeg",
        }
    }
}

/// Image style preference.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ImageStyle {
    #[default]
    Vivid,
    Natural,
}

/// Request to generate a new image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub size: Option<ImageSize>,
    pub quality: Option<ImageQuality>,
    pub format: Option<OutputFormat>,
    pub style: Option<ImageStyle>,
    pub n: Option<u8>,
}

/// Request to edit an existing image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditRequest {
    pub prompt: String,
    pub image_paths: Vec<String>,
    pub mask_path: Option<String>,
    pub model: Option<String>,
    pub size: Option<ImageSize>,
    pub quality: Option<ImageQuality>,
    pub format: Option<OutputFormat>,
    pub n: Option<u8>,
}

/// Normalized response from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub images: Vec<ImageData>,
    pub model: String,
    pub usage: Option<UsageInfo>,
}

/// Raw image data from the provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub b64_json: String,
    pub revised_prompt: Option<String>,
}

/// Token/request usage information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Final result of a completed image job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResult {
    pub file_path: String,
    pub format: OutputFormat,
    pub size_bytes: u64,
    pub revised_prompt: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_size_as_str() {
        assert_eq!(ImageSize::Square.as_str(), "1024x1024");
        assert_eq!(ImageSize::Landscape.as_str(), "1536x1024");
        assert_eq!(ImageSize::Portrait.as_str(), "1024x1536");
        assert_eq!(ImageSize::Auto.as_str(), "auto");
    }

    #[test]
    fn test_output_format_extension() {
        assert_eq!(OutputFormat::Png.extension(), "png");
        assert_eq!(OutputFormat::Webp.extension(), "webp");
        assert_eq!(OutputFormat::Jpeg.extension(), "jpeg");
    }

    #[test]
    fn test_generate_request_serde() {
        let req = GenerateRequest {
            prompt: "A cat sitting on a mat".into(),
            model: None,
            size: Some(ImageSize::Square),
            quality: Some(ImageQuality::Hd),
            format: Some(OutputFormat::Png),
            style: Some(ImageStyle::Vivid),
            n: Some(1),
        };

        let json = serde_json::to_string(&req).unwrap();
        let parsed: GenerateRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.prompt, "A cat sitting on a mat");
        assert_eq!(parsed.size, Some(ImageSize::Square));
    }
}
