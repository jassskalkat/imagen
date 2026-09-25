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
| `get_config` | View the current server configuration (secrets are never leaked) |
| `estimate_cost` | Estimate the cost of a generation/edit operation |
| `list_models` | List available models from the configured provider |

## Prerequisites

- Rust 1.70 or later
- An API key from OpenAI or an Azure OpenAI deployment

## Install

```bash
cargo install --locked --path .
```

Then run `imagen setup` once.

## Configuration

The recommended public flow is:

1. Run `imagen setup`
2. Enter the provider and API key
3. Add the binary to your MCP config

The server stores everything else locally in a config file and loads it automatically.
Environment variables still work and override the saved config when present.
OpenAI is a true one-step setup; Azure also asks for the resource name or endpoint,
and the wizard fills in the standard endpoint/deployment defaults.

### Runtime config file

The setup wizard writes a JSON file to `~/.config/imagen/config.json` by default.
You can override that path with `IMAGEN_CONFIG_FILE`.

### Environment variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `IMAGEN_CONFIG_FILE` | No | `~/.config/imagen/config.json` | Path to the saved config file |
| `IMAGEN_PROVIDER` | No | `openai` | Provider to use: `azure` or `openai` |
| `OPENAI_API_KEY` | Yes (OpenAI) | - | OpenAI API key |
| `OPENAI_ORG_ID` | No | - | OpenAI organization ID |
| `AZURE_OPENAI_ENDPOINT` | Yes (Azure) | - | Azure OpenAI endpoint URL |
| `AZURE_OPENAI_DEPLOYMENT` | Yes (Azure) | - | Azure OpenAI deployment name |
| `AZURE_OPENAI_API_KEY` | Yes (Azure) | - | Azure OpenAI API key |
| `AZURE_OPENAI_API_VERSION` | No | `2025-04-01-preview` | Azure OpenAI API version |
| `IMAGEN_OUTPUT_DIR` | No | `./imagen-output` | Directory for saved image artifacts |
| `IMAGEN_MAX_CONCURRENT_JOBS` | No | `4` | Maximum concurrent generation jobs |
| `IMAGEN_DEFAULT_MODEL` | No | `gpt-image-2.5-sunburst` | Default model for generation |

## Docker

### Build the Image

```bash
docker build -t imagen .
```

### Run the Container

The server communicates over stdio, so you need to run it in interactive mode:

```bash
docker run --rm -i \
  -e IMAGEN_PROVIDER=openai \
  -e OPENAI_API_KEY=sk-your-key-here \
  -v /path/to/output:/data/output \
  imagen
```

For Azure:

```bash
docker run --rm -i \
  -e IMAGEN_PROVIDER=azure \
  -e AZURE_OPENAI_ENDPOINT=https://your-resource.openai.azure.com \
  -e AZURE_OPENAI_DEPLOYMENT=gpt-image-2.5-sunburst \
  -e AZURE_OPENAI_API_KEY=your-azure-key \
  -v /path/to/output:/data/output \
  imagen
```

### Docker Compose Example

```yaml
version: "3.8"
services:
  imagen:
    build: .
    stdin_open: true
    environment:
      IMAGEN_PROVIDER: openai
      OPENAI_API_KEY: ${OPENAI_API_KEY}
      IMAGEN_OUTPUT_DIR: /data/output
      IMAGEN_MAX_CONCURRENT_JOBS: "4"
    volumes:
      - ./output:/data/output
```

## MCP Client Configuration

After `imagen setup`, the MCP client only needs the binary command.

### Claude Desktop

Add to your Claude Desktop MCP config (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "imagen": {
      "command": "/path/to/imagen"
    }
  }
}
```

### VS Code / Cursor

Add to your workspace `.vscode/mcp.json` or user settings:

```json
{
  "servers": {
    "imagen": {
      "command": "/path/to/imagen"
    }
  }
}
```

Or using Docker:

```json
{
  "servers": {
    "imagen": {
      "command": "docker",
      "args": ["run", "--rm", "-i",
        "-e", "IMAGEN_PROVIDER=openai",
        "-e", "OPENAI_API_KEY=sk-your-key-here",
        "-v", "/path/to/output:/data/output",
        "imagen"
      ]
    }
  }
}
```

### Generic MCP Client (stdio transport)

```json
{
  "command": "/path/to/imagen",
  "transport": "stdio"
}
```

## Coding-agent skill

The public coding-agent skill is distributed at
`skills/imagen-mcp-coding-agent/SKILL.md`. It documents OpenAI and Azure
configuration, MCP client setup, the complete tool workflow, polling and edit
sessions, model options, sandbox rules, and validation commands. Distribute the
skill directory without credentials, local config files, or generated images.

## Usage Examples

### Generate an Image

Request the `generate_image` tool with a prompt:

```json
{
  "prompt": "A serene mountain lake at sunset with reflections",
  "size": "1536x1024",
  "quality": "high",
  "output_format": "png",
  "background": "auto",
  "n": 1
}
```

The server returns a job ID and a cost-estimate envelope immediately
(`status: "queued"`). Exact GPT Image cost is provider-usage dependent; the
estimate is `null` until usage is available. The generation runs in the
background — poll `check_job` until the status is `completed` or `failed`.

### Edit an Image

Use `edit_image` to modify an existing image:

```json
{
  "prompt": "Add a wooden dock extending into the lake",
  "image_path": "./imagen-output/source/lake-image.png",
  "size": "1024x1024",
  "quality": "high",
  "output_format": "png"
}
```

Returns a `job_id` and a `session_id` immediately (`status: "queued"`). Poll `check_job` for
the result. Once the job completes, use `session_id` with `continue_edit_session` for
multi-turn edits. The session is marked in-flight as soon as the edit is queued; calling
`continue_edit_session` while the session's previous edit is still in-flight returns an
error telling you to wait for it to complete before continuing.

### Check Job Status

Poll for results with `check_job`:

```json
{
  "job_id": "e4b2c1d8-..."
}
```

Status is one of `queued`, `running`, `completed`, `failed`, or `expired`. Returns artifact
file paths and optional base64 previews once the job reaches `completed`.

## Security

### Path Sandboxing

All file operations (reading input images, writing output artifacts) are sandboxed to the
configured `IMAGEN_OUTPUT_DIR`. Source images and masks must be copied into that directory
before editing. Path traversal attempts, existing files outside the directory, and symlinks
that escape the sandbox are rejected before provider I/O. Masks must be PNG files and are
limited to 4 MiB.

### Secret Safety

The `get_config` tool never exposes API keys or sensitive credentials. All secret fields are
redacted in the response, showing only whether they are configured (present/absent) without
revealing actual values.

### Retry and Timeout Behavior

- HTTP requests to providers have configurable timeouts to prevent hanging connections.
- Transient failures (network errors, rate limits, 5xx responses) are retried automatically
  with exponential backoff and jitter.
- Non-retryable errors (4xx client errors, invalid input) fail immediately.

### Authentication

The server does not implement its own authentication layer. It is designed to be run as a
subprocess managed by an MCP client. The caller is responsible for controlling access to the
server process and securing API keys via environment variables.

## Development

### Building from Source

```bash
cargo build --release
```

The compiled binary is at `target/release/imagen`.

### Running Tests

```bash
cargo test
```

### Linting

```bash
cargo clippy -- -D warnings
```

### Formatting

```bash
# Check formatting
cargo fmt --check

# Auto-format
cargo fmt
```

### Running in Debug Mode

```bash
RUST_LOG=debug cargo run
```

## Known Limitations

- **Stdio-only transport**: The server currently only supports MCP stdio transport. HTTP/SSE
  transport is planned but not yet implemented.
- **In-memory job storage**: Job state is stored in memory and is lost when the server
  process exits. There is no persistence across restarts.
- **Maximum 4 concurrent jobs (default)**: The default concurrency limit is 4, enforced by a
  semaphore that background job tasks must acquire before calling the provider. This can be
  increased via `IMAGEN_MAX_CONCURRENT_JOBS`, but higher values increase memory usage.
- **No built-in authentication**: The server relies on the calling process for access control.
  Do not expose the server process directly to untrusted networks.
- **No built-in rate limiting**: Rate limiting is delegated to the upstream provider. If the
  provider returns 429 (Too Many Requests), the server retries with backoff.

## Troubleshooting

### Missing API Key

**Error**: `OPENAI_API_KEY is required for OpenAI provider`

Ensure the environment variable is set before starting the server:
```bash
export OPENAI_API_KEY=sk-your-key-here
```

### Wrong Provider

**Error**: `Unknown provider: xxx. Expected 'azure' or 'openai'.`

Set `IMAGEN_PROVIDER` to either `azure` or `openai` (case-insensitive).

### Request Timeout

**Symptom**: Jobs fail with a timeout error.

Image generation can take 10-30 seconds depending on the model and parameters. If you
consistently see timeouts, check your network connectivity to the provider endpoint. The
server uses generous default timeouts but retries transient failures automatically.

### Rate Limiting (429 errors)

**Symptom**: Jobs fail after multiple retries.

The server retries 429 responses with exponential backoff. If requests still fail:
- Reduce `IMAGEN_MAX_CONCURRENT_JOBS` to lower parallel requests.
- Check your provider account for rate limit quotas.
- Wait for the rate limit window to reset.

### Output Directory Permission Denied

**Symptom**: Jobs fail with a file I/O error.

Ensure the configured `IMAGEN_OUTPUT_DIR` exists and the server process has write permissions:
```bash
mkdir -p ./imagen-output
chmod 755 ./imagen-output
```

## Planned Features

- **HTTP/SSE transport**: Support for network-based MCP transport alongside stdio.
- **Persistent job storage**: Optional backend (SQLite, Redis) for job state that survives
  restarts.
- **Built-in rate limiting dashboard**: Visibility into request rates and quota usage.
- **Image caching**: Content-addressable cache to avoid re-generating identical prompts.
- **Webhook notifications**: Notify external systems when jobs complete.

## Architecture

```
src/
  main.rs          - Entry point, server initialization, graceful shutdown
  mcp.rs           - MCP server wiring and tool dispatch
  config.rs        - Environment-based configuration
  types.rs         - Shared data types (requests, responses, enums)
  error.rs         - Centralized error types
  cost.rs          - Cost estimation logic
  jobs.rs          - In-memory job registry with lifecycle tracking
  artifacts.rs     - Artifact path generation and file I/O
  retry.rs         - Retry logic with exponential backoff
  sandbox.rs       - Path sandboxing and validation
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
    get_config.rs        - Configuration inspection (secrets redacted)
    estimate_cost.rs     - Cost estimation tool
    list_models.rs       - Model listing
  runtime/
    mod.rs         - Runtime module exports
    state.rs       - Shared AppState with sessions
    worker.rs      - Background job worker
```

## License

MIT
