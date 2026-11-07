# local-mcp

A Rust MCP adapter for local work through the official Codex App Server. ChatGPT
makes the decisions; local-mcp uses the public `command/exec` protocol without
starting a Codex model, thread, or turn. Its Rust CLI also prepares an independent
runtime and manages the official Tunnel client for a selected project.

The supported deployment is macOS, with one selected workspace per connection.
Reads retain host-user access; write permissions keep the existing workspace and
trusted-root limits. Linux/Windows deployment is not qualified.

## Download and first setup

Download your CPU's archive and `SHA256SUMS` from this repository's GitHub Releases
page: `aarch64-apple-darwin` for Apple Silicon or `x86_64-apple-darwin` for Intel.
Verify the archive with `shasum -a 256`, extract it, and put it in your chosen
location. Release archives contain local-mcp, this README and LICENSE. Rust and
Python are not required on the deployed path. The initial releases are unsigned
and not notarized; macOS may require its normal first-open approval.

Separately obtain an official [Codex executable](https://github.com/openai/codex/releases)
with the required command capabilities and the official
[Tunnel client](https://github.com/openai/tunnel-client/releases). local-mcp never
downloads or updates these dependencies. The optional `list_files` and
`search_files` tools also need [ripgrep](https://github.com/BurntSushi/ripgrep).
Install it separately or set an absolute `search_executable` in trusted Settings.
Keep the executable outside all workspace, temporary, extra and additional write
roots so a permitted file edit cannot replace the adapter's read-only backend.
Its path is resolved at session startup; missing or unrecognizable executables
produce `feature_unavailable`. Other tools remain available. Run local-mcp:

```sh
./local-mcp setup
./local-mcp
```

Setup asks for the Tunnel ID, executable locations, Keychain service/account and
a **new** runtime directory. PATH candidates are offered for explicit selection.
It validates Codex's generated command schemas and runs the client's local
`doctor` with a placeholder credential. It neither reads a real key nor starts a
connection. Keep a prepared runtime at its absolute path.

Create or reuse a generic password in macOS Keychain Access: the item name/service
defaults to `chatgpt-mcp-tunnel`, the account defaults to your local username, and
the password is the Tunnel runtime key. Never enter that key in Chat, CLI arguments
or a settings file. Complete the
[official Tunnel and ChatGPT registration steps](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels)
for the same Tunnel ID and ChatGPT workspace. Actual credential access occurs only
when the separate credential helper starts the client; macOS may ask for access.
Setup/doctor success does not establish account authorization or Chat connectivity.

An argument-free first launch runs setup and then workspace selection. Subsequent
launches use the saved runtime and let you choose a recent workspace by number or
enter a path (`~/` is supported). A workspace is never selected automatically.
For noninteractive use, specify the project explicitly:

```sh
./local-mcp connect /absolute/project/path
```

## Daily controls

Keep the Terminal open for the lifetime of the work session. Workspace switching
means ending the session and starting another one.

| Input | Action |
| --- | --- |
| `/help` | List controls |
| `/status` | Workspace, backend, unfinished/unknown jobs, Tunnel readiness |
| `/jobs` | Job IDs, distinct states and known exit codes |
| `/stop JOB_ID` | Request one job's stop using its complete ID; inspect `/jobs` for actual exit |
| `/tunnel-restart` | Reconnect while preserving the original session and jobs |
| `/quit` | Shut down the session and its owned processes |

Commands are local controls, never shell input. Status lines appear at startup,
readiness changes and after controls. Client startup, loopback `/readyz` success,
and a successful tool call from Chat are separate checkpoints. During connection
recovery you can still observe and stop the original jobs. After connecting, ask
Chat to check `session_info` and confirm the selected workspace before doing work.

In a terminal, `/quit` or Ctrl-C with unfinished or unknown jobs asks `[y/N]`.
Enter `y` to proceed, or Enter/`n` to continue work. A second Ctrl-C during this
confirmation requests shutdown immediately. EOF, TERM, HUP and noninteractive
SIGINT also request shutdown without a prompt. Closing the Terminal therefore
ends the session. Stop acceptance does not prove process exit; unconfirmed
shutdown reports retained diagnostics. SIGKILL/power loss cannot run cleanup.
Jobs and output are not restored after restart and commands are never replayed.

## Saved settings and updates

`~/Library/Application Support/local-mcp/preferences.json` stores the default
runtime, trusted Settings and at most ten recent workspace paths. Its directory
is private (0700), the file is private (0600), and writes use a lock and atomic
replacement. It contains no credentials, command bodies or job output. Invalid,
unsupported or insecure preferences stop startup; repair the reported file rather
than silently reverting to weaker settings. A failed recent-list write warns but
does not interrupt an already-started session.

`setup --config settings.json` imports trusted Settings for future connections.
`connect --config settings.json` overrides them for just that launch. Otherwise
connect uses saved Settings or the original defaults. Explicit `start` retains
its original `--config`/defaults behavior. Runtime selection uses `--runtime`,
then the saved default, then the legacy executable-relative runtime directory.

To update, end the old work session and run the **newly downloaded executable**:

```sh
./local-mcp setup --from-runtime /absolute/old/runtime
```

Accept or change the offered settings and choose a new, nonexistent destination.
The Tunnel ID, dependency locations and Keychain references are reused; saved
Settings and recent workspace paths are preserved. Imported root paths are
resolved when saved, so changing the launch directory cannot change their meaning.
The same Tunnel's running
connection blocks activation. The default changes only after preparation and
checks succeed. Failed setup leaves the old default unchanged and may leave the
new directory for inspection. Keep the old runtime for rollback; explicitly run
its launcher or `connect --runtime /absolute/old/runtime` with its original
trusted settings. Running the new executable's setup again from an old runtime
prepares a fresh configuration without deleting either version.
No active files are replaced and no old runtime is deleted automatically.

The release workflow builds and tests both architectures natively, checks that
the `v*` tag matches Cargo's version, and creates a **draft** GitHub Release.
Review and publishing of that draft are separate operations. Nothing is published
or deployed merely by changing this repository's source files.

## Explicit runtime preparation

To prepare the Tunnel runtime in a **new directory**, use:

```sh
local-mcp prepare --destination /new/runtime/directory \
  --tunnel-id tunnel_EXAMPLE \
  --tunnel-client /absolute/path/to/tunnel-client \
  --codex /absolute/path/to/official/codex
```

Preparation copies the binaries, records their SHA256 in `runtime.json`, and
generates a profile and a short Finder launcher. It never reads credentials,
starts services, or overwrites an existing directory. Keep the generated runtime
at its prepared absolute path. Prepare again at a new path to update it.

The existing macOS Keychain item `chatgpt-mcp-tunnel` is the default credential
source. `--keychain-service` and `--keychain-account` select an existing item;
preparation never creates or modifies it. The Tunnel credential helper runs in a
separate child process and execs the official client. The session owner never
receives the key, and the profile removes it before executing the MCP facade.

Start work with `start-local.command /absolute/project/path`, or:

```sh
/new/runtime/directory/bin/local-mcp connect /absolute/project/path \
  --runtime /new/runtime/directory
```

Omit the project to select it interactively. Pass `connect --config settings.json`
for the same trusted execution ceilings as `start --config`. App Server must
initialize successfully before the Tunnel starts. Client startup does **not**
prove remote readiness; the CLI probes the printed health URL's `/readyz`.
`/status` reports the session and client state. `/tunnel-restart` restarts only
connectivity while preserving the original session, App Server, jobs and receipts.
Unexpected client exit permits at most three restarts, after 1, 2 and 4 seconds.
Credential/startup failures await manual intervention. Shutdown controls above
stop the owned client and the session. The same Tunnel ID is exclusive among
new local-mcp launchers; old chatgpt-mcp launchers do not participate in this lock
and must be stopped before switching.

The deployed runtime requires no Python. The test drivers use Python for isolated
fixtures. [Migration and recovery](.scratch/connection-runtime/migration.md)
describe reviewing the new runtime before switching the shared connection.

### Local stdio operation

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

`LOCAL_MCP_SESSION`, set by the trusted Tunnel launcher, restricts the facade to
that session. Every tool requires `session_id`. Call `session_info` first and
include its `runtime_id` in later calls. Old runtime identifiers are rejected.

Ctrl-C or closing start's stdin requests shutdown with the terminal confirmation
rules above. `/status` prints state.
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
| `list_jobs` | Bounded live job IDs, states and exit codes in the current runtime |
| `list_files` | File-name/glob discovery with ignore rules and explicit truncation |
| `search_files` | Literal text search by default; opt-in regex, paths and one-based lines |

Tool definitions describe each argument, units and defaults. Invalid arguments
identify a field and expected value without quoting the invalid content. A
`dispatch: not_attempted` error describes this rejected call, not the historical
outcome of another call with the same request ID. Delivery/response loss still
requires observing the original request before issuing another change. Text
tools publish typed response schemas; images keep their native MCP content.

`read_file` defaults to 200 complete lines and 64 KiB of source text, clamped to
the trusted message budget. `max_lines` and `max_bytes` select a smaller or larger
delivery within that budget. Follow `next_line` when `has_more` is true. A line
larger than the requested budget returns an explicit limit; increase `max_bytes`
within the ceiling or use a CLI byte-range read. Source file size remains unlimited.

`command_status` accepts `max_output_bytes` for the combined stdout/stderr source
bytes. Its default is 16 KiB, clamped to `output_page_bytes`; explicit values must
be at least 8 bytes and no larger than that trusted ceiling. Each stream gets
half. Initial execution/diff responses use the same default. This changes
delivery only, leaving execution, retained output and expiry untouched.

Valid UTF-8 is not split into replacement characters at page boundaries.
`pending_utf8` means an unfinished character awaits another backend notification;
`budget_limited` means the next character cannot fit this page. Original invalid
bytes and retention gaps remain explicit through `lossy` and `gap`.

`list_jobs` defaults to 50 entries (maximum 200). Follow its `next_cursor` with
the same limit/runtime, or omit it to refresh. The list is live, not a snapshot,
and does not include command bodies or recover jobs across owner restarts.

Search defaults to the workspace, single-line literal matching, ignore rules enabled and
hidden files excluded. Set `regex`, `glob`, `hidden`, or `respect_ignore`
explicitly when needed. Searches do not recursively follow symlinks. Results
contain paths and, for text matches, original line text and a one-based `line`.
Search does not transcode UTF-16 or other encodings into UTF-8 matches.
The default limit is 50, maximum 500. `truncated`/`truncation_reason` distinguish
count, delivery, frame and time limits; non-UTF-8 matches/paths are counted as
skipped. No index or snapshot is retained. Search uses fixed argv and a bounded
capability check, with child processes stopped at session shutdown.

`apply_diff` continues to accept Git unified diffs. Its `edits` entries contain
paths, operations and **planned** line counts, separately from `status`:
`planned`, `applied`, `not_applied`, or `unknown`. A check rejection can establish
non-application; an error after apply dispatch can leave partial effects and is
reported as unknown. Git success confirms that invocation, not that the file
will remain unchanged afterwards. Receipts retain this metadata after output
expires. A patch may target at most 256 files; metadata capacity is checked before
dispatch and never causes an old edit to be automatically replayed.

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
survive output loss. `output_error` and `output_unavailable` retain output faults
independently of execution or connection errors. `read_file` has no source-file size limit; oversized delivery
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
| `edit_metadata_bytes_total` | 1 MiB across the session; per-edit delivery also bounded |
| `search_executable` | Optional absolute ripgrep path; otherwise trusted startup PATH |

Each stream uses half the per-job buffer. Text/image delivery budgets are derived
from the message limit, including JSON/base64 expansion. These are configurable
initial resource budgets; they are not transport-provider or performance guarantees.
The owner reserves `max_rpc_inflight` slots each for work (including long status
waits/searches) and controls (session info, job lists, immediate status, input and stop). A separate
pool of that size bounds incoming IPC frame reads; admitted requests retain their
work/control slot through reply delivery. Each MCP facade likewise reserves 64
slots per group. Backend control ACKs have their own `max_rpc_inflight` bound.
Long waits therefore cannot consume the capacity reserved for controls; a group
that is itself full still rejects further requests.
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
python3 tests/cli_ux.py
python3 tests/connection_runtime.py
python3 tests/shutdown.py
python3 -m venv target/schema-tests
target/schema-tests/bin/python -m pip install -r tests/requirements-schemas.txt
target/schema-tests/bin/python tests/schemas.py
cargo build --locked --release
LOCAL_MCP_TEST_BINARY=target/release/local-mcp python3 tests/real_app_server.py /absolute/path/to/codex
```

Rust search integration tests require ripgrep on PATH. The schema validator is a
development-only Python dependency; it is not used by the deployed executable.
CI tests actual MCP responses against published schemas, in addition to the
fault-backend contracts and native-image wire regression.
See the [tool usability implementation record](.scratch/model-tool-usability/implementation.md)
for the accepted changes, final verification results and remaining Chat acceptance.

The opt-in real acceptance script creates disposable files and a temporary
localhost-only HTTP server to test network denial/explicit expansion. It does not
replace a deployed binary or modify ChatGPT, Tunnel, Keychain, or provider settings.
Startup checks schemas generated by the selected executable. Qualification also
requires the real behavior tests; schema presence alone is insufficient. Select
and requalify a CLI explicitly when upgrading. The adapter never downloads or
updates Codex or falls back to internal Rust crates.
