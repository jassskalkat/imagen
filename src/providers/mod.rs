pub mod azure;
pub mod openai;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::types::{EditRequest, GenerateRequest, ProviderResponse};

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
