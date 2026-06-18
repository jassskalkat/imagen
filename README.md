# Imagen MCP Server

A Rust-based [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server that provides
image generation and editing capabilities via Azure OpenAI and OpenAI APIs.

## Features

The server exposes 7 tools over MCP stdio transport:

| Tool | Description |
|------|-------------|
| `generate_image` | Generate images from a text prompt |
| `edit_image` | Edit an existing image with a text prompt and optional mask |
| `continue_edit_session` | Continue a multi-turn edit session on the same image |
| `check_job` | Check the status and retrieve results of a submitted job |
| `get_config` | View the current server configuration |
| `estimate_cost` | Estimate the cost of a generation/edit operation |
| `list_models` | List available models from the configured provider |

## Prerequisites

- Rust 1.70 or later
- An API key from OpenAI or an Azure OpenAI deployment

## Build

```bash
cargo build --release
```

The compiled binary will be at `target/release/imagen`.

## Configuration

All configuration is via environment variables:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `IMAGEN_PROVIDER` | No | `openai` | Provider to use: `azure` or `openai` |
| `OPENAI_API_KEY` | Yes (OpenAI) | - | OpenAI API key |
| `OPENAI_ORG_ID` | No | - | OpenAI organization ID |
| `AZURE_OPENAI_ENDPOINT` | Yes (Azure) | - | Azure OpenAI endpoint URL |
| `AZURE_OPENAI_DEPLOYMENT` | Yes (Azure) | - | Azure OpenAI deployment name |
| `AZURE_OPENAI_API_KEY` | Yes (Azure) | - | Azure OpenAI API key |
| `AZURE_OPENAI_API_VERSION` | No | `2024-06-01` | Azure OpenAI API version |
| `IMAGEN_OUTPUT_DIR` | No | `./imagen-output` | Directory for saved image artifacts |
| `IMAGEN_MAX_CONCURRENT_JOBS` | No | `4` | Maximum concurrent generation jobs |
| `IMAGEN_DEFAULT_MODEL` | No | `gpt-image-2` | Default model for generation |

## MCP Client Configuration

### Claude Desktop

Add to your Claude Desktop MCP config (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "imagen": {
      "command": "/path/to/imagen",
      "env": {
        "IMAGEN_PROVIDER": "openai",
        "OPENAI_API_KEY": "sk-your-key-here",
        "IMAGEN_OUTPUT_DIR": "/path/to/output"
      }
    }
  }
}
```

### Generic MCP Client (stdio transport)

```json
{
  "command": "/path/to/imagen",
  "transport": "stdio",
  "env": {
    "IMAGEN_PROVIDER": "azure",
    "AZURE_OPENAI_ENDPOINT": "https://your-resource.openai.azure.com",
    "AZURE_OPENAI_DEPLOYMENT": "gpt-image-2",
    "AZURE_OPENAI_API_KEY": "your-azure-key",
    "IMAGEN_OUTPUT_DIR": "./output"
  }
}
```

## Usage Examples

### Generate an Image

Request the `generate_image` tool with a prompt:

```json
{
  "prompt": "A serene mountain lake at sunset with reflections",
  "size": "1536x1024",
  "quality": "hd",
  "style": "natural",
  "output_format": "png",
  "n": 1
}
```

The server returns a job ID and cost estimate. Use `check_job` to retrieve results.

### Edit an Image

Use `edit_image` to modify an existing image:

```json
{
  "prompt": "Add a wooden dock extending into the lake",
  "image_paths": ["/path/to/lake-image.png"],
  "size": "1024x1024",
  "quality": "standard"
}
```

### Check Job Status

Poll for results with `check_job`:

```json
{
  "job_id": "e4b2c1d8-..."
}
```

Returns the job status, and when completed, artifact file paths and optional base64 previews.

## Architecture

```
src/
  main.rs          - Entry point, server initialization
  mcp.rs           - MCP server wiring and tool dispatch
  config.rs        - Environment-based configuration
  types.rs         - Shared data types (requests, responses, enums)
  error.rs         - Centralized error types
  cost.rs          - Cost estimation logic
  jobs.rs          - In-memory job registry with lifecycle tracking
  artifacts.rs     - Artifact path generation and file I/O
  providers/
    mod.rs         - ImageProvider trait definition
    openai.rs      - OpenAI API client implementation
    azure.rs       - Azure OpenAI API client implementation
  tools/
    mod.rs         - Tool registration and routing
    generate_image.rs    - generate_image handler
    edit_image.rs        - edit_image handler
    continue_edit_session.rs - Multi-turn edit sessions
    check_job.rs         - Job status checking
    get_config.rs        - Configuration inspection
    estimate_cost.rs     - Cost estimation tool
    list_models.rs       - Model listing
  runtime/
    mod.rs         - Runtime module exports
    state.rs       - Shared AppState with sessions
    worker.rs      - Background job worker
```

## Running Tests

```bash
cargo test
```

## License

MIT
