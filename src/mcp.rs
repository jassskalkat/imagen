use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router, ServerHandler};

use crate::runtime::state::AppState;
use crate::tools::{
    CheckJobInput, ContinueEditSessionInput, EditImageInput, EstimateCostInput, GenerateImageInput,
};

/// The MCP server struct that holds application state and routes tool calls.
#[derive(Debug, Clone)]
pub struct ImagenServer {
    state: Arc<AppState>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl ImagenServer {
    /// Create a new ImagenServer with the given application state.
    pub fn new(state: AppState) -> Self {
        let state = Arc::new(state);
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl ImagenServer {
    /// Generate images from a text prompt using AI.
    #[tool(
        name = "generate_image",
        description = "Queue generation of one or more images from a text prompt. Returns a job ID and cost estimate immediately; the job runs in the background — poll check_job for status and results."
    )]
    async fn generate_image(&self, Parameters(input): Parameters<GenerateImageInput>) -> String {
        match crate::tools::generate_image::run(&self.state, input).await {
            Ok(result) => result,
            Err(e) => serde_json::json!({"error": e}).to_string(),
        }
    }

    /// Edit an existing image using AI with a text prompt.
    #[tool(
        name = "edit_image",
        description = "Queue an edit of an existing image using a text prompt. Optionally provide a mask to control which areas are edited. Returns a job ID and a session ID immediately; the edit runs in the background — poll check_job for status and results before using the session for continue_edit_session."
    )]
    async fn edit_image(&self, Parameters(input): Parameters<EditImageInput>) -> String {
        match crate::tools::edit_image::run(&self.state, input).await {
            Ok(result) => result,
            Err(e) => serde_json::json!({"error": e}).to_string(),
        }
    }

    /// Continue a multi-turn edit session with a new prompt.
    #[tool(
        name = "continue_edit_session",
        description = "Queue the next edit in a multi-turn session, using the last completed image as the source. Returns a job ID immediately; the edit runs in the background — poll check_job for status and results. Fails if the session's previous edit has not finished yet."
    )]
    async fn continue_edit_session(
        &self,
        Parameters(input): Parameters<ContinueEditSessionInput>,
    ) -> String {
        match crate::tools::continue_edit_session::run(&self.state, input).await {
            Ok(result) => result,
            Err(e) => serde_json::json!({"error": e}).to_string(),
        }
    }

    /// Check the status and results of an image generation or edit job.
    #[tool(
        name = "check_job",
        description = "Check the status of a submitted job (queued, running, completed, or failed). Poll this after generate_image, edit_image, or continue_edit_session until the status is completed or failed. Completed jobs include artifact paths and optionally inline base64 image data."
    )]
    async fn check_job(&self, Parameters(input): Parameters<CheckJobInput>) -> String {
        match crate::tools::check_job::run(&self.state, input).await {
            Ok(result) => result,
            Err(e) => serde_json::json!({"error": e}).to_string(),
        }
    }

    /// Get current server configuration (non-sensitive).
    #[tool(
        name = "get_config",
        description = "Get the current server configuration including active provider, output directory, max concurrent jobs, and available models. Never exposes API keys."
    )]
    async fn get_config(&self) -> String {
        match crate::tools::get_config::run(&self.state).await {
            Ok(result) => result,
            Err(e) => serde_json::json!({"error": e}).to_string(),
        }
    }

    /// Estimate the cost of an image generation or edit operation.
    #[tool(
        name = "estimate_cost",
        description = "Estimate the cost of generating or editing images based on size, quality, and count parameters. Estimate only; actual provider billing may differ. Based on published per-image pricing as of this server's release and does not reflect real-time provider pricing changes."
    )]
    async fn estimate_cost(&self, Parameters(input): Parameters<EstimateCostInput>) -> String {
        match crate::tools::estimate_cost::run(&self.state, input).await {
            Ok(result) => result,
            Err(e) => serde_json::json!({"error": e}).to_string(),
        }
    }

    /// List available models from the configured provider.
    #[tool(
        name = "list_models",
        description = "List all available image generation models from the currently configured provider."
    )]
    async fn list_models(&self) -> String {
        match crate::tools::list_models::run(&self.state).await {
            Ok(result) => result,
            Err(e) => serde_json::json!({"error": e}).to_string(),
        }
    }
}

#[tool_handler(
    name = "imagen",
    version = "0.1.1",
    instructions = "MCP server for AI image generation and editing via Azure OpenAI and OpenAI APIs"
)]
impl ServerHandler for ImagenServer {}
