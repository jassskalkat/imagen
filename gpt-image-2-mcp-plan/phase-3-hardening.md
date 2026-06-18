# Phase 3 - Hardening

**Goal:** Make the server reliable enough for repeated asset generation.

## Why this phase exists

This phase closes the gaps that only show up under real use: bad inputs, provider failures, stale jobs, concurrency pressure, and repeated edits.

## Hardening goals

- clear failure modes
- stable logging
- safe file handling
- predictable retries
- no stale session leakage
- no surprise blocking behavior
- no silent provider mismatch

## Hardening checklist

- [ ] Validate bad prompts and missing files.
- [ ] Validate Azure auth failures and rate limits.
- [ ] Validate OpenAI auth failures and rate limits.
- [ ] Validate queue timeouts and stale jobs.
- [ ] Validate file-path safety.
- [ ] Validate cost reporting.
- [ ] Add structured logs.
- [ ] Validate that generated artifacts are readable by the client.
- [ ] Validate that repeated edit sessions do not leak old state.
- [ ] Validate that provider failures are translated into stable MCP errors.

## Hardening task list

- [ ] Add provider error mapping.
- [ ] Add retry policy.
- [ ] Add concurrency caps.
- [ ] Add unit coverage for request building and job transitions.
- [ ] Add smoke coverage for Azure auth and OpenAI auth.
- [ ] Add negative tests for missing files, invalid sizes, and unsupported modes.
- [ ] Add tests for expired edit sessions.
- [ ] Add tests for job polling after completion and after expiry.

## Operational concerns

### Concurrency

Even if the user does not want a hard budget cap, the server should still know how many jobs it can process at once. Concurrency must be explicit so the process stays stable under load.

### Logging

Use structured logs only. Never print secrets, file contents, or raw provider payloads.

### Artifact safety

Generated files should be stored in a deterministic place and should not overwrite unrelated results unless the naming rule explicitly allows it.

### Provider differences

The Azure and OpenAI adapters may differ in:

- endpoint format
- auth format
- response payload shape
- edit-session behavior

That difference should be hidden from the tool layer.

## Failure cases that must be exercised

- missing prompt
- missing source image
- invalid mask file
- unsupported size
- provider 401 / 403
- provider 429
- provider 5xx
- stale job polling
- expired session use
- file path permission issue

## Exit gate

This phase is done only when:

- the server fails clearly
- logs are clean and structured
- the core flows are reliable under repeated agent-driven use
- the provider-specific edge cases are covered by tests

## Blockers

- If a failure mode is still ambiguous, it belongs here, not in rollout.
- If the retry policy is hand-wavy, the server is not hardened enough yet.

