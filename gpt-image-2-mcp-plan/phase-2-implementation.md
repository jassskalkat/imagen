# Phase 2 - Implementation

**Goal:** Build the MCP server and provider adapters.

## Why this phase exists

This is where the plan becomes a working server slice by slice. Tool handlers stay thin; provider logic, job management, and artifact handling remain reusable and testable.

## What this phase should produce

- a Rust MCP server that starts cleanly
- Azure image generation working end-to-end
- OpenAI image generation working end-to-end
- image editing and iterative refinement
- async job submission and polling
- on-disk artifacts with predictable paths
- structured MCP responses that are easy for agents to parse

## Implementation principles

- Keep every touched file below the hard line cap.
- Keep Azure and OpenAI adapters behind the same contract.
- Keep tool handlers as thin orchestration layers.
- Keep serialization stable and explicit.
- Validate inputs early.
- Use structured logs only.

## Surface areas likely to be touched

- `src/main.rs`
- `src/mcp.rs`
- `src/config.rs`
- `src/error.rs`
- `src/jobs.rs`
- `src/artifacts.rs`
- `src/cost.rs`
- `src/types.rs`
- `src/runtime/*`
- `src/providers/*`
- `src/tools/*`

## Implementation checklist

- [ ] Scaffold the Rust workspace.
- [ ] Implement Azure image generation.
- [ ] Implement Azure image editing.
- [ ] Implement OpenAI image generation.
- [ ] Implement OpenAI image editing.
- [ ] Implement async job submission and polling.
- [ ] Persist artifacts to disk.
- [ ] Return structured tool results.
- [ ] Add basic smoke tests.
- [ ] Keep each touched file below the hard line cap.
- [ ] Ensure the Azure and OpenAI adapters share the same tool contract.

## Task list

- [ ] Add `generate_image`.
- [ ] Add `edit_image`.
- [ ] Add `continue_edit_session`.
- [ ] Add `check_job`.
- [ ] Add `estimate_cost`.
- [ ] Add config loading and validation.
- [ ] Add stdout/stderr-safe logging.
- [ ] Add request validation for image count, size, and format.
- [ ] Add fallback handling for provider-specific response differences.
- [ ] Add model/provider discovery if it helps clients choose a deployment.

## Implementation notes

### Generation path

The generation tool should:

1. validate prompt and options
2. route to the correct provider adapter
3. submit the request asynchronously if needed
4. save the resulting file
5. return both the artifact and a structured summary

### Editing path

The editing tool should:

1. validate source images and optional masks
2. normalize all image inputs
3. submit the edit request through the same provider contract
4. preserve edit-session context where applicable
5. return the updated artifact and session state

### Cost reporting

The tool response should include:

- provider
- model
- requested options
- applied options
- estimated cost
- artifact metadata

### Error handling

Map provider failures into stable server errors like:

- invalid prompt
- invalid image file
- unsupported size
- missing auth
- quota/rate limit
- transient provider failure
- stale job
- expired edit session

## Validation during implementation

- Build the server after each major slice.
- Exercise at least one generation path before adding more surface area.
- Verify that artifacts are written where the plan expects them.
- Keep the async job behavior visible and testable.

## Exit gate

This phase is done only when:

- a client can generate and edit an image end-to-end against Azure
- the same contract also works against the OpenAI provider path
- the code stays within the file-size rules
- basic smoke tests exist for the happy path

## Blockers

- If the provider adapters diverge in shape, stop and normalize the contract.
- If a file approaches 400 lines, split it now.
- If the server depends on hidden manual state, that is a phase failure.

