# Rust Phase 1 Architecture

This directory is a fresh, headless Rust migration slice. It deliberately
does not attempt TypeScript feature parity. The TypeScript runtime remains the
shipped product while this workspace establishes a portable core for Hyper or
another frontend.

## Phase 1 shape

```text
CLI / future Hyper adapter
          │ CoreCommand / Event
          ▼
     atomic-core ── LlmProvider ── atomic-provider-openai ── vLLM / cloud
          │
          ├── SessionStore (JSON on local disk)
          └── Tool + ApprovalGate ── atomic-tools
```

`atomic-core` owns the single-inference-per-step control loop. A completion
either returns text or one/more tool calls. The loop asks an injected
`ApprovalGate` before dispatch, appends the result to the transcript, and
continues until a reply or its bounded step limit. `Event` and `CoreCommand`
are serializable, frontend-agnostic boundary types; neither knows about a
terminal nor a webview.

`atomic-provider-openai` is the primary provider adapter. It uses the
OpenAI-compatible `/v1/chat/completions` streaming API, preserving streamed
text and incrementally assembled tool-call arguments. The `LlmProvider` trait
is intentionally small so cloud adapters can be added without changing the
loop. Provider credentials remain out of sessions and event payloads.

`atomic-tools` supplies the Phase 1 workspace-bound file read/write/exact
edit operations, `rg` search, and structured-argument shell execution. The
loop is the dispatch boundary: the CLI's gate denies `shell.run` unless
`--approve-shell` is supplied. Shell commands are never passed through a
shell interpreter, preventing command-string injection; they take a program
name plus an argument array.

## vLLM operation

Run a tool-capable model with a parser appropriate to that model family. For
example, with a Hermes template/parser:

```bash
vllm serve NousResearch/Hermes-2-Pro-Llama-3-8B \
  --host 127.0.0.1 --port 8000 \
  --enable-auto-tool-choice --tool-call-parser hermes \
  --structured-outputs-config.backend xgrammar

cargo run -p atomic-cli -- run \
  --endpoint http://127.0.0.1:8000/v1 \
  --model NousResearch/Hermes-2-Pro-Llama-3-8B \
  --workspace "$PWD" \
  "List the Rust workspace files"
```

For Mistral-family tool models, vLLM documents supplying a compatible chat
template and using `--tool-call-parser mistral`; replace the parser for the
served model instead of copying `hermes` blindly. `--enable-auto-tool-choice`
requires `--tool-call-parser`. Structured outputs are built into the
OpenAI-compatible server; `--structured-outputs-config.backend xgrammar`
selects the current structured-output backend explicitly. Confirm exact
server flags against `vllm serve --help=all` for the installed vLLM release.

Phase 1 does **not** start, download, select, or monitor vLLM. It connects to
an already-running loopback server. Model provisioning, GPU policy,
authentication, health checks, retry/fallback policy, and cloud-provider
configuration are later runtime/config work.

## TypeScript-to-Rust migration map

| TypeScript area | Relevant ownership | Rust destination | Status |
| --- | --- | --- | --- |
| `src/agent` | `agent-loop.ts`, step/dispatch/policy layers | `atomic-core::agent` | Phase 1 vertical slice |
| `src/runtime` | composition, turn lifetime, cancellation | `atomic-core` composition facade | Later |
| `src/llm` | provider protocol, streams, fallbacks, profiles | `atomic-core::LlmProvider`, `atomic-provider-openai` | Phase 1 OpenAI-compatible stream only |
| `src/local-llm` | managed llama server, GGUF assets, slots | no crate | Drop: replaced by external vLLM lifecycle |
| `src/prompt` | stable prefix, prompt packs, grammar framing | `atomic-prompt` | Later |
| `src/tools` | registry, OS tools, output masking | `atomic-tools` | Phase 1 file/shell/rg subset |
| `src/approval` | gate, saved rules, read scope, taint | `ApprovalGate` / `atomic-approval` | Phase 1 injected gate; durable policy later |
| `src/session` | transcript and turn checkpointing | `atomic-core::session` | Phase 1 JSON session store |
| `src/memory` | SQLite/FTS/embeddings/reflection | `atomic-memory` | Later |
| `src/mcp` | client lifecycle, tool discovery, OAuth | `atomic-mcp` | Later |
| `src/config` | schema, migrations, secret storage | `atomic-config` | Later; CLI flags only now |
| `src/cli` | terminal frontend and commands | `atomic-cli` | Phase 1 `run` command |
| `src/http` | authenticated OpenAI/Anthropic ingress | `atomic-http` | Later |
| `src/sidecar` | locked NDJSON command/event bridge | `atomic-sidecar` | Later; core boundary prepared |
| `src/tui` | Ink terminal UI | no Phase 1 crate | Later |
| `src/channels` | Telegram/Discord | `atomic-channels` | Later |
| `src/tasks`, scheduler | durable deferred turns | `atomic-tasks` | Later |
| Fusion | planner/worker pools, worktrees, judging | `atomic-fusion` | Later |
| browser/web/desktop/document tools | Playwright and platform integrations | dedicated crates | Later |
| analytics, tracing, update, uninstall | release/operational services | dedicated crates | Later |

## Explicitly removed from the Rust design

The Rust core has no `llama.cpp` process management, TurboQuant, GGUF/mmproj
asset handling, slot pinning, GBNF grammar generation, llama-server prompt
cache mechanics, or `n_predict`/DRY sampler controls. vLLM owns model
execution and structured decoding. The agent consumes standard OpenAI tool
calls rather than encoding a llama-specific grammar.

## Hard parts and non-goals

- **Browser driving:** a safe browser controller needs a process boundary,
  profile ownership, ARIA/screenshot budgeting, cancellation, and approval
  semantics. It is not a thin Playwright port.
- **TUI:** the current TUI is a large product surface. Rust should consume
  `CoreCommand`/`Event`, not leak frontend state into the control loop.
- **Tauri sidecar:** the NDJSON protocol has concurrency and already-locked
  turn semantics; recreating it comes after command/event compatibility tests.
- **Memory fabric:** SQLite FTS, optional embeddings, graph links, reflection,
  deduplication, and durable rollback are a subsystem, not a JSON field.
- **Fusion:** orchestration, worktree isolation, budgets, approval refusal,
  task recovery, and judging require independent design and tests.
- **Security:** Phase 1 has bounded workspace file paths and an explicit shell
  approval seam, but it is not a sandbox, does not implement secret masking,
  taint review, saved rules, or external read scope.

## Hyper boundary

`atomic-hyper-shell` is intentionally a compiling stub. Cloning
`https://github.com/mast3rmindx/hyper` in this Cloud Agents environment
returned GitHub's `Repository not found`, so no Hyper APIs were invented.
Once an environment exposes both private repositories, the adapter should map
Hyper inputs to `CoreCommand` and render `Event` values as chat timeline,
tool, approval, and error states.
