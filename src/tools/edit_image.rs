use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::jobs::JobKind;
use crate::runtime::state::AppState;
use crate::sandbox::validate_input_path;
use crate::tools::parse::{
    parse_background, parse_compression, parse_moderation, parse_quality, parse_size,
};
use crate::types::EditRequest;

/// Input parameters for the edit_image tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditImageInput {
    /// Path to the source image file to edit.
    pub image_path: String,
    /// Additional image paths for multi-image edits (up to 16 total).
    pub additional_image_paths: Option<Vec<String>>,
    /// Text prompt describing the desired edits.
    pub prompt: String,
    /// Optional path to a mask image (white areas will be edited).
    pub mask_path: Option<String>,
    /// Image size: "1024x1024", "1536x1024", "1024x1536", "auto", or arbitrary "WxH".
    pub size: Option<String>,
    /// Image quality: "low", "medium", "high", "auto", "standard", or "hd".
    pub quality: Option<String>,
    /// Number of images to generate (1-10).
    pub n: Option<u8>,
    /// Output compression percentage (0-100).
    pub output_compression: Option<u8>,
    /// Image background: "opaque" or "auto".
    pub background: Option<String>,
    /// Content moderation level: "low" or "auto".
    pub moderation: Option<String>,
    /// User tracking identifier passed to the API.
    pub user: Option<String>,
}

/// Output from the edit_image tool.
#[derive(Debug, Serialize)]
pub struct EditImageOutput {
    pub job_id: String,
    pub session_id: String,
    pub status: String,
}

/// Execute the edit_image tool logic.
pub async fn run(state: &AppState, input: EditImageInput) -> Result<String, String> {
    if input.prompt.trim().is_empty() {
        return Err("Prompt cannot be empty.".to_string());
    }
    if input.image_path.trim().is_empty() {
        return Err("image_path cannot be empty.".to_string());
    }

    // Build combined image paths
    let mut image_paths = vec![input.image_path.clone()];
    if let Some(ref additional) = input.additional_image_paths {
        image_paths.extend(additional.iter().cloned());
    }
    if image_paths.len() > 16 {
        return Err("Total number of images cannot exceed 16.".to_string());
    }

    // Validate n
    let n = input.n.unwrap_or(1);
    if n == 0 || n > 10 {
        return Err("n must be between 1 and 10.".to_string());
    }

    // Validate compression
    let output_compression = match input.output_compression {
        Some(c) => Some(parse_compression(c).map_err(|e| e.to_string())?),
        None => None,
    };

    // Validate all input paths for path traversal and null bytes
    for path in &image_paths {
        validate_input_path(path).await.map_err(|e| e.to_string())?;
    }
    if let Some(ref mask) = input.mask_path {
        validate_input_path(mask).await.map_err(|e| e.to_string())?;
    }

    // Validate all image files exist
    for path in &image_paths {
        if !tokio::fs::metadata(path)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            return Err(format!("Image file not found: '{path}'"));
        }
    }

    // Validate mask file if provided
    if let Some(ref mask) = input.mask_path {
        if !tokio::fs::metadata(mask)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            return Err(format!("Mask file not found: '{mask}'"));
        }
    }

    let size = match &input.size {
        Some(s) => Some(parse_size(s).map_err(|e| e.to_string())?),
        None => None,
    };
    let quality = match &input.quality {
        Some(q) => Some(parse_quality(q).map_err(|e| e.to_string())?),
        None => None,
    };
    let background = match &input.background {
        Some(b) => Some(parse_background(b).map_err(|e| e.to_string())?),
        None => None,
    };
    let moderation = match &input.moderation {
        Some(m) => Some(parse_moderation(m).map_err(|e| e.to_string())?),
        None => None,
    };

    let provider_name = state.provider.provider_name();
    let model = state.config.default_model.clone();

    // Create job and immediately mark as Running to prevent the background
    // worker from picking it up (avoids double-execution race).
    let job_id = state
        .job_registry
        .create_job(JobKind::Edit, provider_name, &model, &input.prompt)
        .await;
    let _ = state
        .job_registry
        .update_status(&job_id, crate::jobs::JobStatus::Running)
        .await;

    // Create a new edit session
    let session_id = Uuid::new_v4().to_string();

    // Build edit request
    let request = EditRequest {
        prompt: input.prompt,
        image_paths: image_paths.clone(),
        mask_path: input.mask_path,
        model: Some(model),
        size,
        quality,
        format: None,
        n: Some(n),
        output_compression,
        background,
        moderation,
        user: input.user,
    };

    // Submit to provider
    let result = state.provider.edit(&request).await;
    match result {
        Ok(response) => {
            let mut results = Vec::new();
            let mut last_path = input.image_path.clone();
            for (i, img) in response.images.iter().enumerate() {
                let bytes = base64::Engine::decode(
                    &base64::engine::general_purpose::STANDARD,
                    &img.b64_json,
                )
                .map_err(|e| format!("Failed to decode image data: {e}"))?;

                let fmt = crate::types::OutputFormat::default();
                let path = crate::artifacts::artifact_path(
                    &state.config.output_dir,
                    &job_id,
                    i as u32,
                    &fmt,
                );
                crate::artifacts::save_artifact(&path, &bytes, &state.config.output_dir)
                    .await
                    .map_err(|e| format!("Failed to save artifact: {e}"))?;

                last_path = path.to_string_lossy().to_string();
                results.push(crate::types::ImageResult {
                    file_path: last_path.clone(),
                    format: fmt,
                    size_bytes: bytes.len() as u64,
                    revised_prompt: img.revised_prompt.clone(),
                });
            }
            let _ = state.job_registry.complete_job(&job_id, results).await;
            // Record session with the last produced image
            state.upsert_edit_session(&session_id, &last_path).await;
        }
        Err(e) => {
            let _ = state.job_registry.fail_job(&job_id, e.to_string()).await;
            // Still record session with original image
            state
                .upsert_edit_session(&session_id, &input.image_path)
                .await;
        }
    }

    let output = EditImageOutput {
        job_id,
        session_id,
        status: "submitted".to_string(),
    };

    serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, Provider};
    use crate::error::Result;
    use crate::providers::{ImageProvider, ModelInfo};
    use crate::types::{EditRequest, GenerateRequest, ImageData, ProviderResponse};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct MockProvider;

    #[async_trait]
    impl ImageProvider for MockProvider {
        async fn generate(&self, _request: &GenerateRequest) -> Result<ProviderResponse> {
            Ok(ProviderResponse {
                images: vec![],
                model: "mock".to_string(),
                usage: None,
            })
        }
        async fn edit(&self, _request: &EditRequest) -> Result<ProviderResponse> {
            Ok(ProviderResponse {
                images: vec![ImageData {
                    b64_json: "dGVzdA==".to_string(),
                    revised_prompt: None,
                }],
                model: "mock".to_string(),
                usage: None,
            })
        }
        fn get_models(&self) -> Vec<ModelInfo> {
            vec![]
        }
        fn provider_name(&self) -> &'static str {
            "mock"
        }
    }

    fn test_state() -> AppState {
        let config = AppConfig {
            provider: Provider::OpenAI,
            azure_endpoint: None,
            azure_deployment_name: None,
            azure_api_key: None,
            azure_api_version: None,
            openai_api_key: Some("sk-test".into()),
            openai_org_id: None,
            output_dir: std::env::temp_dir()
                .join("imagen-edit-test")
                .to_string_lossy()
                .to_string(),
            max_concurrent_jobs: 2,
            default_model: "gpt-image-2".into(),
        };
        AppState::new(config, Arc::new(MockProvider))
    }

    fn make_input(image_path: &str, prompt: &str) -> EditImageInput {
        EditImageInput {
            image_path: image_path.to_string(),
            additional_image_paths: None,
            prompt: prompt.to_string(),
            mask_path: None,
            size: None,
            quality: None,
            n: None,
            output_compression: None,
            background: None,
            moderation: None,
            user: None,
        }
    }

    #[tokio::test]
    async fn test_empty_prompt_returns_error() {
        let result = run(&test_state(), make_input("/tmp/image.png", "   ")).await;
        assert!(result.unwrap_err().contains("Prompt cannot be empty"));
    }

    #[tokio::test]
    async fn test_empty_image_path_returns_error() {
        let result = run(&test_state(), make_input("", "Edit this")).await;
        assert!(result.unwrap_err().contains("image_path cannot be empty"));
    }

    #[tokio::test]
    async fn test_nonexistent_image_path_returns_error() {
        let input = make_input("/tmp/nonexistent-imagen-test-image-xyz.png", "Edit this");
        let err = run(&test_state(), input).await.unwrap_err();
        assert!(
            err.contains("not found") || err.contains("does not exist"),
            "Expected 'not found' error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_nonexistent_mask_path_returns_error() {
        let tmp_image = std::env::temp_dir().join("imagen-edit-test-img.png");
        tokio::fs::write(&tmp_image, b"fake image data")
            .await
            .unwrap();

        let mut input = make_input(&tmp_image.to_string_lossy(), "Edit this");
        input.mask_path = Some("/tmp/nonexistent-imagen-test-mask-xyz.png".to_string());
        let err = run(&test_state(), input).await.unwrap_err();
        assert!(
            err.contains("not found") || err.contains("does not exist"),
            "Expected 'not found' error, got: {err}"
        );
        let _ = tokio::fs::remove_file(&tmp_image).await;
    }

    #[tokio::test]
    async fn test_too_many_images_returns_error() {
        let mut input = make_input("/tmp/img.png", "Edit this");
        input.additional_image_paths = Some(vec!["/tmp/img.png".to_string(); 16]);
        let result = run(&test_state(), input).await;
        assert!(result.unwrap_err().contains("cannot exceed 16"));
    }

    #[tokio::test]
    async fn test_n_eleven_returns_error() {
        let mut input = make_input("/tmp/img.png", "Edit this");
        input.n = Some(11);
        let result = run(&test_state(), input).await;
        assert!(result.unwrap_err().contains("n must be between 1 and 10"));
    }
}
