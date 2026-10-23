# local-mcp

A Rust MCP adapter for local work through the official Codex App Server. ChatGPT
makes the decisions; local-mcp uses the public `command/exec` protocol without
starting a Codex model, thread, or turn. The existing chatgpt-mcp wrapper, Tunnel,
Keychain and connection lifecycle remain responsible for the MCP connection.

The required acceptance platform is macOS. This revision does not provide a
qualified Linux/Windows deployment.

## Run

Install an official Codex CLI with the required public command capabilities,
then build this repository with Rust 1.96:

```sh
cargo build --locked --release
cd /path/to/project
/path/to/local-mcp start my-project --codex /absolute/path/to/codex
```

In the existing MCP stdio connection, run:

```sh
local-mcp mcp
```

The `start` process owns the original App Server stdio connection, jobs, output
and request receipts. MCP facade processes can disconnect and reconnect while
that owner and backend stay alive. There is no recovery across owner/backend
restarts, automatic command replay, or additional daemon/connection path.

`LOCAL_MCP_SESSION`, when set by the existing wrapper, restricts the facade to
that session. Every tool requires `session_id`. Call `session_info` first and
include its `runtime_id` in later calls. Old runtime identifiers are rejected.

Ctrl-C or closing start's stdin requests shutdown. `/status` prints state.
Duplicate starts cannot replace an existing socket/lock. Following a crash,
inspect the previous owner before manually removing its stale socket and lock
from the printed per-user socket directory. No PID guessing or automatic
recovery occurs.

## Trusted configuration

Pass `start --config /path/to/settings.json`. The file is a strict JSON object;
omitted settings retain their defaults. MCP arguments cannot change it.

```json
{
  "max_jobs": 4,
  "write_roots": [],
  "extra_roots": [],
  "allow_network": false,
  "allow_full_access": false,
  "allow_git_metadata": false,
  "allow_ssh_agent": false
}
```

The project directory and a private session temporary directory are basic write
roots. `write_roots` adds explicitly trusted basic roots. `extra_roots` is a
ceiling for roots that must also be explicitly selected for an expanded job.
Git metadata candidates are displayed; `allow_git_metadata` includes them only
when explicitly enabled at startup. Reads retain host-user access outside write
roots. Canonical cwd must be inside the job's roots, except for explicitly
allowed full access.

Normal commands have no network. Expanded commands select limited extra
roots/network, or a separate full-access mode that removes both filesystem and
network sandbox restrictions. Every expansion defaults off. Chat host operation
confirmation and these Mac-side execution ceilings are distinct; `approved`,
yolo and tool annotations are not authorization.

Commands receive only HOME, trusted PATH, locale, TERM, TZ and private TMPDIR.
The existing same-user SSH agent is exposed only with `allow_ssh_agent`.
Connection keys, backend CODEX_HOME and unrelated inherited variables are excluded.
Use explicit argv, including an explicit shell or `env` command when needed.

## Tools

| Tool | Purpose |
| --- | --- |
| `session_info` | Runtime, roots, ceilings, effective limits and backend state |
| `read_file` | UTF-8 text with original newlines and optional one-based line range |
| `view_image` | Native MCP image content, with macOS decoder validation |
| `apply_diff` | Git check followed by one apply, without index changes |
| `exec_command` | Basic argv execution; default 30-second observation, `wait_ms: 0` for immediate job |
| `exec_command_expanded` | One execution with explicit `reason` and `access` within trusted ceilings |
| `command_status` | Exactly one job/request ID, exit result and retained output with a cursor |
| `write_stdin` | Exactly one UTF-8 `data`, `eof: true`, or PTY `resize` |
| `stop_command` | One stop attempt on the original backend connection |

Execution, diff and input require a client `request_id`. Same ID and content
return the original receipt/state. Changed content is rejected. A new ID is a
new operation. Receipts and known exit metadata last for the owner's lifetime,
even after output expires. Full receipt capacity rejects new changes before
dispatch; observing existing work and requesting possible stops remain available.

stdin defaults closed; choose `pipe` or `pty` explicitly. Input ACK proves backend
acceptance, not application consumption. Unknown delivery blocks further input
for that job. Stop ACK does not prove exit; observe the final exec result.
Nonzero exit is a successful tool response containing a command failure result.
Backend stdio loss marks unfinished jobs unknown and prevents new execution.
Never recreate output by rerunning an uncertain command.

Output cursors are non-consuming and bound to a runtime/job and byte positions.
stdout/stderr stay separate, except PTY multiplexing into stdout. Buffer gaps,
source truncation, lossy UTF-8 display and expiry are explicit. Known exit results
survive output loss. `read_file` has no source-file size limit; oversized delivery
returns a continuation or an explicit limit, preserving UTF-8 and CRLF.
Images retain PNG/JPEG/GIF/WebP/BMP/TIFF/AVIF support. Actual visual use in ordinary
ChatGPT still requires host acceptance testing; MIME/byte delivery alone is not
visual acceptance.

## Resource limits and diagnostics

New in-memory retention needs finite limits. They are trusted startup settings,
not changes to the existing 30-second foreground wait or unlimited execution
lifetime. The backend explicitly receives `disableTimeout` and `disableOutputCap`.

| Setting | Initial value |
| --- | --- |
| `max_jobs` | 4, configurable |
| `max_receipts` | 8192 |
| `output_bytes_per_job` / `output_bytes_total` | 4 MiB / 16 MiB |
| `output_page_bytes` | 256 KiB, divided between streams |
| `output_ttl_secs` | 3600 after completion |
| `max_message_bytes` | 16 MiB; configurable 64 KiB–64 MiB |
| `max_input_bytes` / `max_diff_bytes` | 256 KiB / 1 MiB |
| `max_image_pixels` | 32 Mi pixels, checked before raster allocation |
| `max_rpc_inflight` | 64 |
| `rpc_timeout_ms` / `shutdown_ms` | 10000 / 5000 |

Each stream uses half the per-job buffer. Text/image delivery budgets are derived
from the message limit, including JSON/base64 expansion. These are configurable
initial resource budgets; they are not transport-provider or performance guarantees.
See the [implementation verification record](.scratch/app-server-adapter/implementation.md)
for the tested workload and remaining acceptance work.

The adapter does not persist command bodies, output or receipts. Official backend
diagnostics can contain bodies and live in a displayed, isolated private directory.
No auth cache is copied and no model login is required for command execution.
Unneeded telemetry exporters are disabled. Only a confirmed normal owner/backend
shutdown removes the owned runtime directory. Crashes and forced backend shutdown
retain residue and report its location. `history.persistence=none` is not a claim
that every official diagnostic store excludes command text.

## Verify

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/real_app_server.py /absolute/path/to/codex
```

The opt-in real acceptance script creates disposable files and a temporary
localhost-only HTTP server to test network denial/explicit expansion. It does not
replace a deployed binary or modify ChatGPT, Tunnel, Keychain, or provider settings.
Startup checks schemas generated by the selected executable. Qualification also
requires the real behavior tests; schema presence alone is insufficient. Select
and requalify a CLI explicitly when upgrading. The adapter never downloads or
updates Codex or falls back to internal Rust crates.
