# Imagen coding-agent skill

`imagen-mcp-coding-agent/SKILL.md` is the public skill for coding agents that
use the Imagen MCP server. Distribute the directory as-is through the agent
skill mechanism; do not include credentials, local config files, or generated
images.

Repository: https://github.com/trailflow-systems-inc/imagen

## Install

1. Install the Imagen binary and connect it to the coding agent as a local MCP
   server.
2. Download or clone this repository.
3. Upload or copy the complete `skills/imagen-mcp-coding-agent/` folder to the
   agent's skills directory. The file must remain named `SKILL.md`.
4. Enable the skill and keep the Imagen MCP connection enabled.
5. Ask the agent: `Set up Imagen with my Azure OpenAI deployment and generate a
   16:9 PNG.`

The skill folder must not contain credentials, `config.json`, generated images,
or a README file. `skills/README.md` is repository-level documentation for
human users and is not part of the uploaded skill folder.

The skill covers:

- OpenAI and Azure OpenAI setup.
- MCP stdio client configuration.
- The complete generate, poll, edit, and continue-edit workflow.
- GPT Image model options and output handling.
- Azure deployment/API-version requirements.
- Path, mask, credential, and retry safety.
- Rust validation commands.

## Validation

The skill package includes `imagen-mcp-coding-agent/evals/evals.json` with
positive and negative trigger cases plus functional workflow cases. Static
skill validation does not prove automatic activation, provider authentication,
Azure deployment availability, or image quality; test those lanes separately
with approved credentials and an isolated output directory.

Run the bundled structural validator from the repository root:

```bash
python3 skills/imagen-mcp-coding-agent/scripts/validate.py
```
