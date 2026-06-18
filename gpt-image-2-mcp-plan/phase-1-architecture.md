# Phase 1 - Architecture

**Goal:** Finalize the Rust architecture before any implementation starts.

## Why this phase exists

This phase locks the module boundaries, tool contracts, runtime model, and provider abstraction. The goal is to make each file small, each tool deterministic, and each provider swap isolated.

## Architecture intent

We are not building a generic LLM gateway. We are building a focused MCP server that:

- accepts prompts and image paths from MCP clients
- calls Azure OpenAI or official OpenAI through a normalized adapter
- stores generated assets on disk
- returns stable tool responses that a coding agent can use immediately
- handles long-running work asynchronously

## Architecture principles

- **Provider-neutral tool contract:** feature code must not care which provider is behind the request.
- **Small files:** every touched file should stay below 400 lines.
- **Thin tool handlers:** business logic lives in provider/job/runtime modules.
- **Async by default:** generation and editing should not block the transport.
- **Deterministic artifacts:** output paths should be predictable and easy to inspect.
- **Explicit errors:** provider failures must be translated into stable MCP errors.

## Confirmed boundaries

### Server-owned

- Azure auth and endpoint calls
- OpenAI auth and endpoint calls
- request shaping and validation
- cost estimation
- job queue / polling state
- file output and artifact naming
- retries and provider error translation
- provider capability discovery
- normalized error mapping
- rate-limit and concurrency enforcement

### Client-owned

- prompt entry
- picking source images
- previewing results
- re-running jobs
- local workflow integration

### Boundary decision

The MCP server owns everything that talks to Azure or OpenAI or touches files. The client only passes prompts, image paths, and follow-up instructions.

## Provider contract

The feature code should only know about a normalized interface like:

- `generate(prompt, options) -> job_id`
- `edit(images, prompt, mask, options) -> job_id`
- `poll(job_id) -> status/result`
- `estimate_cost(request) -> cost summary`

Provider-specific details stay inside the Azure/OpenAI adapter layer.

## Planned Rust layout

```text
src/
  main.rs
  config.rs
  error.rs
  mcp.rs
  artifacts.rs
  cost.rs
  jobs.rs
  types.rs
  providers/
    mod.rs
    azure.rs
    openai.rs
  tools/
    mod.rs
    generate_image.rs
    edit_image.rs
    continue_edit_session.rs
    check_job.rs
    get_config.rs
    estimate_cost.rs
    list_models.rs
  runtime/
    mod.rs
    worker.rs
    state.rs
```

## Runtime model

- **stdio transport** is the default for local MCP clients.
- **HTTP/SSE** is for broader integrations and future remote use.
- **Jobs** should be queued and polled so slow image generation never blocks the server process.
- **Artifacts** should be written to a deterministic per-workspace folder.
- **Edit sessions** should keep enough state to support refinement loops without forcing the caller to resend everything.

## Tool surface

### Required tools

- `generate_image`
- `edit_image`
- `continue_edit_session`
- `check_job`
- `get_config`
- `estimate_cost`

### Optional tools

- `stream_image` if the client path benefits from it
- `list_models` if it helps clients discover which providers/deployments are configured
- `get_last_result` if that materially improves iterative workflows

## Data model sketch

### Job state

Jobs should have a small explicit state machine, likely:

- queued
- running
- completed
- failed
- expired

### Artifact shape

Each artifact should keep at least:

- stable ID
- file path
- MIME type
- size
- provider
- model
- prompt reference
- timestamps

### Edit session shape

If sessions are in-memory for v1, they still need:

- session ID
- latest image reference
- step count
- last update time
- retention rules

## Architecture checklist

- [ ] Choose `rmcp` for MCP transport.
- [ ] Choose the Azure provider adapter implementation.
- [ ] Define the internal image contract.
- [ ] Define the job lifecycle and status model.
- [ ] Define the artifact layout and naming rules.
- [ ] Define error categories and retry behavior.
- [ ] Define the cost-estimation model.
- [ ] Define the job state machine and which states are terminal.
- [ ] Define exactly which tool inputs are required vs optional.
- [ ] Define what gets returned inline vs only as a file path.
- [ ] Define how provider capability discovery is exposed.
- [ ] Define how concurrency limits are enforced.

## Architecture task list

- [ ] Draft the tool schemas.
- [ ] Draft the provider trait.
- [ ] Draft the job registry shape.
- [ ] Draft the config schema and env vars.
- [ ] Draft the module split so no file exceeds 400 lines.
- [ ] Draft the normalized provider response shape.
- [ ] Draft the MCP response envelopes for success and error cases.
- [ ] Draft the cost-output summary format.

## Exit gate

This phase is done only when:

- the server shape is source-backed and fixed
- the provider boundary is explicit
- the tool schemas are stable enough to implement without redesign
- the runtime model is clear enough that implementation can start without hidden assumptions

## Blockers

- If the provider response contract is unclear, do not start implementation.
- If file splitting would be violated by the proposed layout, split now, not later.

