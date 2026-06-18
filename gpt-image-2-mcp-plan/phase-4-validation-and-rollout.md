# Phase 4 - Validation and Rollout

**Goal:** Prove the MCP works in the target clients and is safe to roll out.

## Why this phase exists

This is the final checkpoint before the plan is operationally ready. The server must work in real MCP clients, generate real artifacts, and be documented well enough that another developer can reproduce setup without guessing.

## Validation checklist

- [ ] Run end-to-end image generation from the target MCP client.
- [ ] Run one edit flow.
- [ ] Run one multi-turn refinement flow.
- [ ] Verify generated files appear where expected.
- [ ] Verify cost estimates are shown.
- [ ] Verify no secrets are written to disk.
- [ ] Record the rollout and rollback steps.
- [ ] Confirm the OpenAI and Azure configs are documented clearly enough for another developer to reproduce.
- [ ] Confirm the output files are easy to locate and inspect.

## Rollout task list

- [ ] Capture final smoke test commands.
- [ ] Capture the known-good Azure configuration.
- [ ] Capture the known-good OpenAI configuration.
- [ ] Capture the support/runbook notes.
- [ ] Capture the follow-up backlog.
- [ ] Capture the exact client config snippets for stdio and HTTP/SSE.
- [ ] Capture the known limitations and unsupported cases.
- [ ] Capture the expected cost behavior for common sizes and qualities.

## What “done” means here

The server should be usable by:

- a coding agent in a local harness
- a desktop MCP client
- a developer debugging image generation

The release notes and setup instructions should be sufficient for external open-source users, not just the author.

## Release evidence to collect

- a working generate call
- a working edit call
- a working multi-turn edit session
- one Azure-backed example
- one OpenAI-backed example
- artifact paths on disk
- cost estimate output
- client config snippets

## Operational handoff

The final documentation should make these obvious:

- how to configure Azure
- how to configure OpenAI
- which env vars are required
- where generated assets go
- what the known limits are
- how to roll back or disable the server if needed

## Exit gate

This phase is done only when:

- the server is ready for controlled internal use
- the release notes and setup instructions are sufficient for open-source users
- the final validation evidence is saved and readable

## Blockers

- If setup cannot be reproduced from the written docs, rollout is not ready.
- If the client config is still ambiguous, do not call the plan complete.

