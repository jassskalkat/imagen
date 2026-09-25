---
name: imagen-mcp-coding-agent
description: Configures and guides coding agents using the Imagen Rust stdio MCP to generate, edit, and iteratively refine GPT Images through OpenAI or Azure OpenAI. Use when a user asks to set up Imagen MCP, configure Azure OpenAI or OpenAI, generate an image, edit a PNG/JPEG/WebP, poll an image job, troubleshoot MCP errors, or validate the Rust runtime.
license: MIT
compatibility: Requires the local imagen binary, an MCP stdio client, and provider credentials supplied through environment variables or a secret manager. Provider calls require network access.
metadata:
  author: Trailflow Systems
  version: "1.0.0"
  category: image-generation
  runtime: rust-stdio-mcp
  mcp-server: imagen
  tags: [mcp, coding-agents, image-generation, azure-openai, openai, rust]
---

# Imagen MCP for Coding Agents

Use this skill when a coding agent needs to generate or edit images through the
Imagen MCP server. The server runs locally over MCP stdio and exposes the same
tool contract for OpenAI and Azure OpenAI.

## Safety rules

- Never print, commit, paste, or transmit API keys, access tokens, or config
  files containing credentials.
- Keep `IMAGEN_OUTPUT_DIR` private and writable only by the MCP process.
- Copy source images into `IMAGEN_OUTPUT_DIR` before calling `edit_image`.
- Use only image paths inside `IMAGEN_OUTPUT_DIR`; Imagen rejects traversal,
  symlink escapes, and existing files outside the directory.
- Masks must be PNG files and no larger than 4 MiB.
- Do not claim a job completed until `check_job` returns `status: "completed"`.
- Treat `estimate_cost.estimated_cost_usd: null` as unknown, not free.
- Keep prompts and generated images appropriate for the provider's policy.

## Installation

From the Imagen repository:

```bash
cargo install --locked --path .
```

The binary must be available as `imagen`, or use its absolute path in the MCP
client configuration.

## Scope and trigger boundaries

Use this skill for Imagen MCP setup and image-generation workflows. Do not use
it for general Rust questions, arbitrary image manipulation without the Imagen
server, image-policy decisions, or provider billing/account administration.

## Core use cases and success criteria

### Generate an image

Trigger examples:

- "Generate a hero image with Imagen."
- "Use Azure OpenAI to create a 16:9 illustration."
- "Create three concept images and save the results."

Success means the provider is configured, the request is accepted, the returned
job is polled without a busy loop, and `check_job` reports `completed` with
artifact paths.

### Edit and refine an image

Trigger examples:

- "Edit this PNG to remove the background."
- "Use Imagen to add a dock to this image."
- "Continue refining the previous image."

Success means every input is inside the sandbox, the edit job is terminal before
the next session step, and the final artifact path is reported.

### Configure or troubleshoot the MCP

Trigger examples:

- "Set up Imagen with my Azure deployment."
- "Why is Imagen returning DeploymentNotFound?"
- "Validate the Imagen MCP installation."

Success means the agent checks `get_config`, verifies the provider/deployment
contract, avoids exposing credentials, and reports static validation separately
from any live provider verification.

## OpenAI mode

Use the local setup wizard:

```bash
imagen setup
```

Or configure the environment explicitly:

```bash
export IMAGEN_PROVIDER=openai
export OPENAI_API_KEY='use-a-secret-manager'
export IMAGEN_DEFAULT_MODEL=gpt-image-2.5-sunburst
export IMAGEN_OUTPUT_DIR="$PWD/imagen-output"
```

Available OpenAI image models exposed by `list_models` include:

- `gpt-image-2.5-sunburst`: highest-fidelity generation and editing.
- `gpt-image-2.5-flare`: faster everyday generation.
- `gpt-image-2`: high-resolution generation and editing.
- `dall-e-3`: generation only; it cannot edit and is retired for new Azure
  deployments.

## Azure OpenAI mode

Azure uses a deployed GPT Image model. `AZURE_OPENAI_DEPLOYMENT` is the Azure
deployment name, not necessarily the model ID. Configure the resource endpoint,
deployment, API key, and API version:

```bash
export IMAGEN_PROVIDER=azure
export AZURE_OPENAI_ENDPOINT='https://YOUR_RESOURCE.openai.azure.com'
export AZURE_OPENAI_DEPLOYMENT='YOUR_GPT_IMAGE_DEPLOYMENT'
export AZURE_OPENAI_API_KEY='use-a-secret-manager'
export AZURE_OPENAI_API_VERSION='2025-04-01-preview'
export IMAGEN_DEFAULT_MODEL=gpt-image-2.5-sunburst
export IMAGEN_OUTPUT_DIR="$PWD/imagen-output"
```

The provider calls:

```text
POST {endpoint}/openai/deployments/{deployment}/images/generations?api-version={api_version}
POST {endpoint}/openai/deployments/{deployment}/images/edits?api-version={api_version}
```

Azure authentication uses the `api-key` header. The Azure deployment must
support the configured GPT Image model and the selected API version. If the
deployment name differs from the model ID, keep `AZURE_OPENAI_DEPLOYMENT` set
to the deployment name and keep `IMAGEN_DEFAULT_MODEL` set to the model's
capability name.

Official Azure references:

- https://learn.microsoft.com/en-us/azure/foundry/openai/how-to/dall-e
- https://learn.microsoft.com/en-us/azure/foundry/openai/reference-preview
- https://learn.microsoft.com/en-us/azure/foundry/openai/reference

Official OpenAI references:

- https://developers.openai.com/api/docs/guides/image-generation
- https://developers.openai.com/api/docs/guides/tools-image-generation
- https://developers.openai.com/api/reference/resources/images

## MCP client configuration

Generic stdio configuration:

```json
{
  "mcpServers": {
    "imagen": {
      "command": "imagen",
      "transport": "stdio"
    }
  }
}
```

Copilot-style local configuration:

```json
{
  "imagen": {
    "type": "local",
    "command": "imagen",
    "tools": ["*"]
  }
}
```

Prefer environment variables or a secret manager for credentials. Do not put
`OPENAI_API_KEY` or `AZURE_OPENAI_API_KEY` in a checked-in MCP JSON file.

After connecting, call `get_config` before attempting generation. A successful
MCP connection only proves protocol transport; it does not prove provider
authentication or deployment availability.

## Required tool workflow

1. Call `get_config` to confirm the provider, default model, output directory,
   concurrency, and available models. It does not return secrets.
2. Call `list_models` when the provider or model is uncertain.
3. Call `estimate_cost` before expensive work. It returns a safe envelope; the
   dollar value can be `null` because current GPT Image pricing depends on
   provider usage.
4. Call `generate_image` with a prompt and optional:
   - `size`: `auto`, `1024x1024`, `1536x1024`, `1024x1536`, or a valid custom
     GPT Image resolution.
   - `quality`: `auto`, `low`, `medium`, `high`, `xhigh`, or `max`. `xhigh`
     and `max` are for GPT Image 2.5 models.
   - `output_format`: `png`, `webp`, or `jpeg`.
   - `output_compression`: `0`–`100` for WebP/JPEG.
   - `background`: `auto`, `opaque`, or `transparent`. Transparent output
     requires PNG or WebP.
   - `n`: `1`–`10`.
5. Save the returned `job_id`, then poll `check_job` every few seconds. Stop
   on `completed`, `failed`, or `expired`; do not busy-loop.
6. Read completed artifact paths from `check_job.artifacts`. Small artifacts
   may include an inline `base64_preview`; use the file path for larger images.

Example:

```json
{
  "prompt": "A clean editorial illustration of a rust-colored fox reading code beside a terminal window",
  "size": "1536x1024",
  "quality": "high",
  "output_format": "png",
  "background": "opaque",
  "n": 1
}
```

## Examples

### Azure generation

User says: "Generate a high-quality 16:9 image with my Azure Imagen setup."

Actions:

1. Call `get_config` and confirm `provider` is `azure`.
2. Call `list_models` and confirm the configured capability matches the Azure
   deployment.
3. Call `generate_image` with `size: "1536x1024"`, `quality: "high"`, and
   `output_format: "png"`.
4. Poll `check_job` until terminal and return the artifact path.

Result: A completed image artifact, or a provider-specific error with the
credential and deployment values kept private.

### Iterative local edit

User says: "Add a watercolor effect, then make it warmer."

Actions:

1. Copy the source image into `IMAGEN_OUTPUT_DIR`.
2. Call `edit_image` and wait for its job to complete.
3. Pass its `session_id` to `continue_edit_session` with the second prompt.
4. Poll the second job and return the final artifact.

Result: A completed second artifact without racing the first edit or reading
files outside the sandbox.

## Editing workflow

1. Put the source image inside `IMAGEN_OUTPUT_DIR`.
2. Call `edit_image` with `image_path`, `prompt`, and the desired output
   options. Add `additional_image_paths` for multi-image composition.
3. If using a mask, provide a PNG path inside `IMAGEN_OUTPUT_DIR`; it must be
   no larger than 4 MiB.
4. Poll the returned `job_id` with `check_job`.
5. For iterative edits, keep the returned `session_id` and call
   `continue_edit_session` only after the previous job is terminal. A session
   rejects concurrent edits.

The runtime uses the Image API edit endpoint and keeps session state in the
running MCP process. Restarting the process expires in-memory jobs and edit
sessions.

## Tool failure handling

### MCP connection failure

If the client cannot start or discover tools:

1. Run `command -v imagen` and confirm the binary is executable.
2. Run `imagen --help` outside the MCP client.
3. Confirm the client uses stdio and does not redirect the server's stdout.
4. Keep logs on stderr; stdout is reserved for MCP protocol messages.
5. Reconnect the client after changing environment variables.

If `get_config` works but generation fails, the MCP transport is healthy and
the remaining issue is provider configuration, quota, request validation, or
the image policy response.

- `Provider authentication failed`: verify the provider, endpoint, deployment,
  API key, and Azure API version without exposing the secret.
- `DeploymentNotFound`: check the exact Azure deployment name; it is not
  automatically inferred from the model ID.
- `Rate limit exceeded`: wait for the retry policy, reduce concurrency with
  `IMAGEN_MAX_CONCURRENT_JOBS`, or use the provider's quota controls.
- `Path ... outside the configured output directory`: copy the input into
  `IMAGEN_OUTPUT_DIR` and retry.
- `Session expired`: start a new `edit_image` call.

## Validation for coding agents

Run from the repository root:

```bash
python3 skills/imagen-mcp-coding-agent/scripts/validate.py
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --locked --release
```

Never use a real provider key in unit tests. For a live smoke test, use a
temporary provider configuration and remove generated artifacts and credentials
afterward.

Static validation does not prove automatic skill activation, live MCP
authentication, Azure deployment availability, provider policy acceptance, or
image quality. Record those as separate unexecuted or live-tested lanes.
