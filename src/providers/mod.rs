pub mod azure;
pub mod openai;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ImagenError;
use crate::error::Result;
use crate::types::{
    EditRequest, GenerateRequest, ImageBackground, ImageQuality, ImageSize, ImageStyle,
    OutputFormat, ProviderResponse,
};

/// Information about a model offered by a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub supports_editing: bool,
    pub max_images: u8,
}

/// Trait defining the interface for image generation providers.
///
/// Both Azure OpenAI and OpenAI implement this trait, allowing
/// the runtime to work with either provider interchangeably.
#[async_trait]
pub trait ImageProvider: Send + Sync {
    /// Generate images from a text prompt.
    async fn generate(&self, request: &GenerateRequest) -> Result<ProviderResponse>;

    /// Edit an existing image with a text prompt.
    async fn edit(&self, request: &EditRequest) -> Result<ProviderResponse>;

    /// Return the list of models available from this provider.
    fn get_models(&self) -> Vec<ModelInfo>;

    /// Return the provider name (e.g., "azure" or "openai").
    fn provider_name(&self) -> &'static str;
}

/// Return whether a model is one of OpenAI's GPT Image models.
pub fn is_gpt_image_model(model: &str) -> bool {
    model.starts_with("gpt-image-") || model == "chatgpt-image-latest"
}

/// Return whether a model supports the current GPT Image 2.5 quality tiers.
pub fn is_gpt_image_25_model(model: &str) -> bool {
    model.starts_with("gpt-image-2.5-")
}

/// Return whether a model is DALL-E 3.
pub fn is_dall_e_3(model: &str) -> bool {
    model == "dall-e-3"
}

pub struct RequestOptions<'a> {
    pub model: &'a str,
    pub size: Option<&'a ImageSize>,
    pub quality: Option<&'a ImageQuality>,
    pub format: &'a OutputFormat,
    pub background: Option<&'a ImageBackground>,
    pub style: Option<&'a ImageStyle>,
    pub n: u8,
    pub output_compression: Option<u8>,
    pub is_edit: bool,
}

/// Validate request options against the selected model before queueing work.
pub fn validate_request_options(options: RequestOptions<'_>) -> Result<()> {
    let RequestOptions {
        model,
        size,
        quality,
        format,
        background,
        style,
        n,
        output_compression,
        is_edit,
    } = options;

    if n == 0 || n > 10 {
        return Err(ImagenError::InvalidInput(
            "n must be between 1 and 10.".to_string(),
        ));
    }

    if is_dall_e_3(model) {
        if is_edit {
            return Err(ImagenError::InvalidInput(
                "dall-e-3 does not support image editing.".to_string(),
            ));
        }
        if n != 1 {
            return Err(ImagenError::InvalidInput(
                "dall-e-3 supports only n=1.".to_string(),
            ));
        }
        if let Some(size) = size {
            match size {
                ImageSize::Square
                | ImageSize::Landscape
                | ImageSize::Portrait
                | ImageSize::Auto => {}
                ImageSize::Custom(_) => {
                    return Err(ImagenError::InvalidInput(
                        "dall-e-3 supports only its standard image sizes.".to_string(),
                    ));
                }
            }
        }
        if let Some(quality) = quality {
            if !matches!(
                quality,
                ImageQuality::Auto | ImageQuality::Standard | ImageQuality::Hd
            ) {
                return Err(ImagenError::InvalidInput(
                    "dall-e-3 quality must be auto, standard, or hd.".to_string(),
                ));
            }
        }
        if format != &OutputFormat::Png {
            return Err(ImagenError::InvalidInput(
                "dall-e-3 does not support output_format; use png.".to_string(),
            ));
        }
        if background.is_some() || output_compression.is_some() {
            return Err(ImagenError::InvalidInput(
                "background and output_compression are only supported by GPT Image models."
                    .to_string(),
            ));
        }
        if style.is_none() {
            return Ok(());
        }
        return Ok(());
    }

    if !is_gpt_image_model(model) {
        return Err(ImagenError::InvalidInput(format!(
            "Unsupported image model '{model}'. Use a GPT Image model or dall-e-3."
        )));
    }

    if let Some(quality) = quality {
        let supported = matches!(
            quality,
            ImageQuality::Auto | ImageQuality::Low | ImageQuality::Medium | ImageQuality::High
        ) || (is_gpt_image_25_model(model)
            && matches!(quality, ImageQuality::XHigh | ImageQuality::Max));
        if !supported {
            return Err(ImagenError::InvalidInput(format!(
                "Quality '{}' is not supported by {model}.",
                quality.as_api_str()
            )));
        }
    }

    if style.is_some() {
        return Err(ImagenError::InvalidInput(
            "style is only supported by dall-e-3.".to_string(),
        ));
    }

    if matches!(background, Some(ImageBackground::Transparent))
        && matches!(format, OutputFormat::Jpeg)
    {
        return Err(ImagenError::InvalidInput(
            "transparent backgrounds require png or webp output.".to_string(),
        ));
    }

    if output_compression.is_some() && matches!(format, OutputFormat::Png) {
        return Err(ImagenError::InvalidInput(
            "output_compression is only supported for jpeg or webp output.".to_string(),
        ));
    }

    Ok(())
}

/// Map a logical size to the selected model's API size.
pub fn api_size(model: &str, size: &ImageSize) -> String {
    if is_dall_e_3(model) {
        return match size {
            ImageSize::Landscape => "1792x1024".to_string(),
            ImageSize::Portrait => "1024x1792".to_string(),
            _ => "1024x1024".to_string(),
        };
    }

    size.as_str().to_string()
}

/// Map the internal quality default to the selected model's API contract.
pub fn api_quality(model: &str, quality: &ImageQuality) -> &'static str {
    if is_dall_e_3(model) && matches!(quality, ImageQuality::Auto) {
        "standard"
    } else {
        quality.as_api_str()
    }
}

impl ImageQuality {
    pub fn as_api_str(&self) -> &'static str {
        match self {
            ImageQuality::Auto => "auto",
            ImageQuality::Standard => "standard",
            ImageQuality::Hd => "hd",
            ImageQuality::Low => "low",
            ImageQuality::Medium => "medium",
            ImageQuality::High => "high",
            ImageQuality::XHigh => "xhigh",
            ImageQuality::Max => "max",
        }
    }
}

impl ImageBackground {
    pub fn as_api_str(&self) -> &'static str {
        match self {
            ImageBackground::Transparent => "transparent",
            ImageBackground::Opaque => "opaque",
            ImageBackground::Auto => "auto",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt_image_25_accepts_extended_quality_and_transparency() {
        assert!(validate_request_options(RequestOptions {
            model: "gpt-image-2.5-sunburst",
            size: Some(&ImageSize::Custom("1920x1088".into())),
            quality: Some(&ImageQuality::XHigh),
            format: &OutputFormat::Webp,
            background: Some(&ImageBackground::Transparent),
            style: None,
            n: 2,
            output_compression: Some(80),
            is_edit: false,
        })
        .is_ok());
    }

    #[test]
    fn gpt_image_rejects_style_and_jpeg_transparency() {
        assert!(validate_request_options(RequestOptions {
            model: "gpt-image-2",
            size: None,
            quality: Some(&ImageQuality::Max),
            format: &OutputFormat::Png,
            background: None,
            style: None,
            n: 1,
            output_compression: None,
            is_edit: false,
        })
        .is_err());
        assert!(validate_request_options(RequestOptions {
            model: "gpt-image-2",
            size: None,
            quality: None,
            format: &OutputFormat::Jpeg,
            background: Some(&ImageBackground::Transparent),
            style: None,
            n: 1,
            output_compression: None,
            is_edit: false,
        })
        .is_err());
    }

    #[test]
    fn dall_e_3_maps_auto_quality_and_rejects_gpt_only_options() {
        assert_eq!(api_quality("dall-e-3", &ImageQuality::Auto), "standard");
        assert_eq!(api_size("dall-e-3", &ImageSize::Landscape), "1792x1024");
        assert!(validate_request_options(RequestOptions {
            model: "dall-e-3",
            size: Some(&ImageSize::Landscape),
            quality: Some(&ImageQuality::Auto),
            format: &OutputFormat::Png,
            background: None,
            style: Some(&ImageStyle::Natural),
            n: 1,
            output_compression: None,
            is_edit: false,
        })
        .is_ok());
        assert!(validate_request_options(RequestOptions {
            model: "dall-e-3",
            size: None,
            quality: None,
            format: &OutputFormat::Webp,
            background: None,
            style: None,
            n: 1,
            output_compression: None,
            is_edit: false,
        })
        .is_err());
    }
}
