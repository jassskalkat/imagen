# GPT-Image-2 MCP Plan

**Owner:** @jassskalkat  
**Primary outcome:** A Rust MCP server that generates and edits images through Azure OpenAI `gpt-image-2` and the official OpenAI image API.  
**Release posture:** Open-source first, then internal alpha, private preview, and limited rollout.

## Why this exists

We need a fast, reliable MCP server that coding agents can use to generate realistic assets without having to care whether the underlying provider is Azure OpenAI or official OpenAI. The server must feel simple from the client side, but internally it needs a clean provider boundary because Azure and OpenAI auth/endpoints differ.

## Core decisions

- **Language/runtime:** Rust
- **MCP SDK:** `rmcp`
- **Transports:** stdio + HTTP/SSE
- **Providers:** Azure OpenAI + official OpenAI
- **Output:** local file artifact + inline image content when useful
- **Job model:** async generation/editing with polling
- **Cost policy:** show estimated cost, but do not hard-block usage

## Confirmed facts

- `gpt-image-2` is GA on Azure Foundry / Azure OpenAI.
- `gpt-image-2` supports text-to-image and image-to-image workflows.
- OpenAI’s docs split image workflows into Image API and Responses API.
- Azure’s `/openai/v1` accepts API keys.
- Rust has an official MCP SDK (`rmcp`), so this can be a real MCP implementation rather than a wrapper script.

## Working assumptions

- Azure OpenAI and official OpenAI should share the same tool contract.
- The server should expose the same tool names regardless of provider.
- The server should save artifacts locally and also return them inline when useful.
- Long-running requests should be async so the MCP connection stays responsive.
- The first version should optimize for generic MCP clients, not only one desktop app.

## Unresolved questions

- Whether partial-image streaming is worth the complexity for v1.
- Whether edit sessions should survive process restarts or stay memory-only.
- Which OpenAI-compatible request fields should be surfaced directly versus hidden behind simplified tool params.

## Scope

### In scope

- `generate_image`
- `edit_image`
- `continue_edit_session`
- `check_job`
- `get_config`
- `estimate_cost`
- `list_models`
- `get_last_result`
- streaming only if it proves worth the added complexity

### Deferred

- web UI
- gallery / asset management
- org-wide quotas and billing dashboards
- video generation

### Non-goals

- replacing Azure OpenAI itself
- building a generic LLM gateway
- supporting every image model on day 1
- synchronous long-running calls that risk MCP timeouts
- automatic prompt rewriting beyond what the provider already does

## File layout target

```text
phase-0-discovery.md
phase-1-architecture.md
phase-2-implementation.md
phase-3-hardening.md
phase-4-validation-and-rollout.md
```

## Quality rules

- Keep files under 400 lines.
- Split logic before a file approaches 300 lines.
- Keep provider code behind a trait or adapter boundary.
- Keep tool handlers thin; move real work into provider and job modules.
- Prefer small modules over deep generic frameworks.
- Keep MCP schemas simple enough that a coding agent can invoke them without guessing hidden fields.
- Put stable domain types in `types.rs` and leave transport glue in `mcp.rs` / `tools/`.

## Phase files

- [Phase 0 - Discovery](./phase-0-discovery.md)
- [Phase 1 - Architecture](./phase-1-architecture.md)
- [Phase 2 - Implementation](./phase-2-implementation.md)
- [Phase 3 - Hardening](./phase-3-hardening.md)
- [Phase 4 - Validation and rollout](./phase-4-validation-and-rollout.md)

