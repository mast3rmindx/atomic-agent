# Rust Migration Roadmap

Status is intentionally conservative. “Later” means absent from this
workspace, not partially complete.

| Area | Status | Notes |
| --- | --- | --- |
| Cargo workspace | Complete for Phase 1 | Five small crates; Rust 1.83-compatible lockfile |
| Agent loop | Complete vertical slice | Bounded sequential tool loop; no recovery, batching, cancellation, or policy modes |
| OpenAI-compatible provider | Complete vertical slice | Streaming chat-completions text/tool deltas; no retries, model catalog, embeddings, or fallback chain |
| vLLM connectivity | Complete connection contract | Documents external server launch only; no managed vLLM process or model provisioning |
| File tools | Complete vertical slice | Workspace-relative UTF-8 read/write/exact edit with size limits |
| Shell and ripgrep | Complete vertical slice | Structured shell argv and `rg`; shell is denied by default in the CLI |
| Approval | Partial | Injectable dispatch gate exists; no interactive waiter, persistent rules, read scope, or taint review |
| Session persistence | Complete vertical slice | Atomic JSON snapshots with restrictive Unix mode; no locks, retention, titles, search, or recovery markers |
| CLI | Complete vertical slice | One `run` command; no interactive REPL, TUI, config file, or service mode |
| UI boundary | Complete interface seed | Serializable `CoreCommand` and `Event`; no controller/runtime host yet |
| Hyper adapter | Stubbed | Private repository was unreachable from this environment; no unverified API use |
| Prompt construction | Later | Must preserve vLLM-native tool semantics without importing GBNF concepts |
| Memory fabric | Later | Requires an explicit SQLite/FTS/vector migration and safety/retention design |
| MCP | Later | Requires transport, dynamic schema validation, OAuth, and approval integration |
| HTTP / sidecar | Later | Requires authenticated ingress, cancellation, streaming, and FIFO turn ownership |
| Browser / web | Later | Requires a secure controller boundary and first-class approval/read-scope design |
| Tasks / scheduler | Later | Requires durable turn checkpoints, worker ownership, and restart semantics |
| Fusion | Later | Requires separate planner/worker orchestration, worktree, budget, and judge design |
| Cloud provider adapters | Later | The trait supports them; none are implemented |

## Next implementation gates

1. Add an interactive approval host that turns `CoreCommand::ResolveApproval`
   into a one-shot decision without allowing a UI to bypass dispatch.
2. Add cancellation, per-session turn locking, and persisted interrupted-turn
   records before exposing long-running UI or HTTP turns.
3. Design the prompt/tool schema layer against real vLLM model/template
   combinations; add compatibility fixtures for native and streamed calls.
4. Add configuration plus secret storage before adding cloud providers.
5. Obtain a Cloud Agents environment with access to `mast3rmindx/hyper`, then
   implement and test a thin adapter against its real APIs.

## Top risks

- Tool calling depends on the vLLM server, model chat template, and selected
  parser agreeing. A parser name that works for one model family is not a
  universal setting.
- A JSON session store is adequate for this slice but not safe for concurrent
  frontends or crash-resume guarantees.
- Centralizing approval in the loop establishes the seam, but it needs durable
  policy and output-taint controls before the tool set expands.
- Rust tool parity, especially browser automation and the memory fabric, is
  substantially more complex than porting TypeScript interfaces.
