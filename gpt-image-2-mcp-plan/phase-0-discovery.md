# Phase 0 - Discovery

**Goal:** Lock the product shape and confirm the Azure/OpenAI deployment details before the Rust architecture is frozen.

## Why this phase exists

This phase eliminates ambiguity. If the provider shape, output mode, transport strategy, or edit-session behavior is wrong here, the implementation will split in the wrong direction and force refactoring later.

## Primary question this phase answers

What exactly is v1 of the MCP server, and what provider/runtime constraints must it obey?

## Context

The server is intended for coding agents and harnesses that need to generate or refine assets during development. The workflow is:

1. Prompt the server for an image.
2. Receive a generated artifact.
3. Inspect the image.
4. Edit or iterate.
5. Save and hand off the asset to the downstream code/design workflow.

The server should be fast in the sense that it has low startup overhead, async provider calls, deterministic tool responses, and no hidden manual steps.

## Source-backed facts that matter here

- `gpt-image-2` is GA on Azure Foundry / Azure OpenAI.
- Azure `/openai/v1` accepts API keys.
- The OpenAI image docs support both single-shot generation and multi-turn workflows.
- Azure samples keep the deployment name explicit; they do not autodiscover it.

## Discovery checklist

- [ ] Confirm whether v1 must support edit flows or generation only.
- [ ] Confirm whether results should be file-only, inline-only, or both.
- [ ] Confirm whether the MCP must be stdio-only or also support HTTP/SSE.
- [ ] Confirm expected asset sizes and cost display behavior.
- [ ] Confirm whether edit sessions should persist across process restarts.
- [ ] Confirm whether streaming is a must-have or a later optimization.
- [ ] Confirm whether the first release should be Azure-only, OpenAI-only, or dual-provider.
- [ ] Confirm the minimum set of image output controls to expose in v1.

## Discovery task list

- [ ] Record the Azure resource name, deployment name, and region.
- [ ] Record the official OpenAI API key path and endpoint preference.
- [ ] Record the intended client targets (any MCP-capable coding agent or harness).
- [ ] Record the preferred output format for generated assets.
- [ ] Record the acceptable concurrency / queue depth.
- [ ] Record the retry and timeout expectations for Azure and OpenAI calls.
- [ ] Record the minimum set of image output controls to expose in v1.

## Scope boundaries for discovery

### Must answer now

- Provider support
- Transport support
- Output mode
- Edit-session persistence
- Streaming requirement

### Can wait until architecture

- exact module names
- exact worker implementation
- final retry algorithm
- specific file splits

## Questions to resolve

### Provider shape

Should the first version treat Azure and official OpenAI as equal first-class providers, or should one be the primary path and the other a compatibility path?

### Transport shape

Should the server ship with stdio only for the first cut, or do we need HTTP/SSE immediately so other harnesses can use it without a local process wrapper?

### Output shape

Should MCP tool results return file paths only, base64 only, or both? The current direction favors both because agents can hand off the file and also inspect the inline result.

### Session shape

Should edit sessions persist across restart, or is in-memory session state enough for v1?

## Exit gate

This phase is done only when:

- the provider contract can be written without hidden assumptions
- the client-facing scope is unambiguous
- the output mode is decided
- the transport decision is fixed
- the edit-session behavior is fixed

## Blockers

- If the Azure/OpenAI compatibility story is still unclear, the architecture phase must not start.
- If the transport decision is unclear, the MCP shape cannot be frozen.

