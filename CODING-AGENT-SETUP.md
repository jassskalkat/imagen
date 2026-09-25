# Imagen MCP setup for coding agents

This guide connects a coding agent to Imagen so it can generate, edit, and
refine images through Azure OpenAI. The MCP runs locally over stdio; it is not
a hosted server or container service.

## 1. Install Imagen

From the repository root:

```bash
cargo install --locked --path .
command -v imagen
```

## 2. Configure Azure OpenAI

Run the secure setup wizard:

```bash
imagen setup
```

Choose **Azure OpenAI**, then enter:

- Azure endpoint: `https://YOUR_RESOURCE.openai.azure.com`
- Deployment name: your deployed GPT Image model
- API key: retrieve it from a secret manager or Azure Key Vault
- API version: `2025-04-01-preview`

The wizard stores the local config at
`~/.config/imagen/config.json` with restrictive permissions. Never put the API
key in an MCP client config, source file, commit, or skill package.

For non-interactive environments, set the same values as environment variables:

```bash
export IMAGEN_PROVIDER=azure
export AZURE_OPENAI_ENDPOINT='https://YOUR_RESOURCE.openai.azure.com'
export AZURE_OPENAI_DEPLOYMENT='YOUR_GPT_IMAGE_DEPLOYMENT'
export AZURE_OPENAI_API_VERSION='2025-04-01-preview'
export AZURE_OPENAI_API_KEY='load-from-your-secret-manager'
export IMAGEN_DEFAULT_MODEL='gpt-image-2.5-sunburst'
```

Use a writable output directory:

```bash
export IMAGEN_OUTPUT_DIR="$HOME/.local/share/imagen/output"
mkdir -p "$IMAGEN_OUTPUT_DIR"
```

## 3. Register the MCP

Add a local stdio server named `imagen`. Use the syntax for your agent:

**Generic, Copilot, or compatible clients**

```json
{
  "mcpServers": {
    "imagen": {
      "type": "local",
      "command": "/Users/YOUR_USER/.local/bin/imagen",
      "tools": ["*"]
    }
  }
}
```

**OpenCode**

```json
{
  "mcp": {
    "servers": {
      "imagen": {
        "type": "local",
        "command": ["/Users/YOUR_USER/.local/bin/imagen"]
      }
    }
  }
}
```

**Kilo Code**

```json
{
  "mcp": {
    "imagen": {
      "type": "local",
      "command": ["/Users/YOUR_USER/.local/bin/imagen"]
    }
  }
}
```

**Pi**

Add the same server to Pi's shared MCP file, usually
`~/.config/mcp/mcp.json`:

```json
{
  "mcpServers": {
    "imagen": {
      "command": "/Users/YOUR_USER/.local/bin/imagen"
    }
  }
}
```

Restart the agent or reload its MCP configuration after editing. Keep stdout
reserved for MCP protocol messages; Imagen logs to stderr.

## 4. Install the skill

Copy the skill folder into the coding agent's skill directory:

```bash
cp -R skills/imagen-mcp-coding-agent "$HOME/.agents/skills/"
python3 skills/imagen-mcp-coding-agent/scripts/validate.py
```

Keep the skill folder's `SKILL.md`, `evals/`, and `scripts/` files together.
Do not copy credentials, `config.json`, or generated images into it.

## 5. Use the MCP

Start with this prompt:

> Use Imagen MCP with my Azure setup. Check the configuration, generate a
> high-quality 16:9 PNG, and poll until the artifact is complete.

The agent should:

1. Call `get_config` and confirm `provider: "azure"`.
2. Use `list_models` if the deployment capability is uncertain.
3. Call `generate_image`.
4. Save the returned `job_id`.
5. Poll `check_job` until `completed`, `failed`, or `expired`.
6. Return the completed artifact path.

For editing, copy the source image into `IMAGEN_OUTPUT_DIR`, call `edit_image`,
and wait for completion before calling `continue_edit_session` with its
`session_id`. Masks must be PNG files no larger than 4 MiB.

## Common fixes

| Symptom | Fix |
|---|---|
| `DeploymentNotFound` | Use the exact Azure deployment name, not only the model ID. |
| `Provider authentication failed` | Check the endpoint, key, deployment, and API version without printing the key. |
| Path outside output directory | Copy the source image into `IMAGEN_OUTPUT_DIR`. |
| MCP cannot start | Run `command -v imagen` and `imagen --help`, then reload the agent. |
| Job remains queued | Keep the MCP process running and poll `check_job`; do not start a second server. |

Official references:

- Azure image generation: https://learn.microsoft.com/en-us/azure/foundry/openai/how-to/dall-e
- Azure image REST reference: https://learn.microsoft.com/en-us/azure/foundry/openai/reference-preview
- OpenAI image generation: https://developers.openai.com/api/docs/guides/image-generation

