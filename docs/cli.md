# CLI and MCP

The `beweft` CLI and its stdio MCP server use the same HTTP API as the web
application. They are part of the current development build; self-host packaging
and direct provider access are still being integrated. A successful fixture test
does not verify a social account or public publication.

Build the binary with `cargo build -p beweft-cli`, then run
`target/debug/beweft --help`. An owner creates API tokens in the application;
choose the minimum scopes and brands needed. Store a token in a private file
readable only by its owner, then set:

```sh
export BEWEFT_URL=http://127.0.0.1:3000
export BEWEFT_TOKEN_FILE=/absolute/path/to/private-token
```

`BEWEFT_TOKEN` is an alternative to the file, but do not set both. The token
file must be a regular UTF-8 file of at most 4096 bytes and have no group or
other permissions (`chmod 600`). Keep it outside source control and agent logs.
The origin must be HTTPS except on loopback. `BEWEFT_ALLOW_INTERNAL_HTTP=true`
is an explicit exception for a trusted private container network; HTTP does not
encrypt the token. Redirects are never followed.

Run `beweft schema <tool>` to see each command's inputs, and supply a JSON
object through `--json FILE` or `--json -` for standard input. The examples
assume the binary is on your `PATH`:

```sh
beweft brands
beweft draft-create --json - <<'JSON'
{"body":{"brand_id":"00000000-0000-0000-0000-000000000000","text":"Example draft"}}
JSON
beweft post --json - <<'JSON'
{"id":"00000000-0000-0000-0000-000000000000"}
JSON
```

Replace example IDs with actual IDs. `draft-create` creates a draft only;
`draft-update` requires the current version. The `api METHOD /path` escape
hatch is limited to the same API origin and bearer scopes.

Before using `schedule`, read the post and selected channels, review the
content and determine the effective media for each selected channel. A
variant's media list overrides the post's base list, including an empty list.
Use `media` to find each effective media item's `metadata_version`. The named
command requires an explicit `media_versions` object with the complete union
of those IDs and versions; use `{}` when there is no effective media. The
server checks the set and versions atomically when it creates snapshots.

Generate a UUID yourself and **persist it together with the exact approved
post ID, brand ID and body before sending**. Do not reconstruct the body from
current media or change the timestamp on retry. `schedule` also requires an
explicit `intent`: `queue` omits `scheduled_at` and reserves the next
posting-rule slot; `exact` supplies a future RFC3339 timestamp with an offset
and at most six fractional digits. Use `schedules` with `channel_id` to inspect
the posting rules. Scheduling can publish when the job is due.

For example, save the following as `schedule.json` after replacing all example
IDs, version, time and media map with the reviewed values:

```json
{"id":"00000000-0000-0000-0000-000000000001","intent":"exact","body":{"version":1,"channel_ids":["00000000-0000-0000-0000-000000000002"],"scheduled_at":"2099-01-02T12:34:56.123456-05:30","request_id":"00000000-0000-0000-0000-000000000003","media_versions":{}}}
```

```sh
beweft schedules --json - <<'JSON'
{"channel_id":"00000000-0000-0000-0000-000000000002"}
JSON
beweft schedule --json schedule.json
beweft schedule-request --json - <<'JSON'
{"request_id":"00000000-0000-0000-0000-000000000003"}
JSON
```

For an unresolved request that should no longer schedule, `schedule-retire`
takes the same saved `request_id` as a top-level argument and a body with
`post_id`, `brand_id`, `version`, `channel_ids`, `media_versions` and, for an
exact schedule, the same `scheduled_at`. It returns the existing receipt if
one already exists. A successful retirement of an unused request creates a
terminal `schedule_request_retired` rejection with no publications, so a
delayed original cannot schedule. **Retiring an accepted request does not
cancel its publications.** A lost retirement response is also checked using
`schedule-request`.

For the saved example above, the retirement input would be:

```json
{"request_id":"00000000-0000-0000-0000-000000000003","body":{"post_id":"00000000-0000-0000-0000-000000000001","brand_id":"00000000-0000-0000-0000-000000000004","version":1,"channel_ids":["00000000-0000-0000-0000-000000000002"],"scheduled_at":"2099-01-02T12:34:56.123456-05:30","media_versions":{}}}
```

Save that object as `retire.json` using the original saved values, then run
`beweft schedule-retire --json retire.json` only when explicitly retiring the
request.

The `schedule` result is a compact summary for each committed publication,
including its ID, state and timing. Use `publication` to read its complete
immutable payload. The server's ordinary schedule response remains full for
clients that do not request the summary view.

`drafts`, `media`, `queue` and `history` return one page at a time, with up to
10 records by default. Their JSON arguments accept `brand_id`, `limit` (1–10)
and `cursor`. Results have a `data` array and `next_cursor`, which is null on
the last page. Supply that cursor with the same filters to read the next page;
the tools do not aggregate a library in memory. Pages are newest-created first,
including queue pages, so use `scheduled_at` for scheduled order. A page is a
live view, not a snapshot: refresh from the start after concurrent changes.

Draft list items include text and media IDs but omit channel variants and
options; use `post` for a complete draft. Queue and history items omit the
immutable payload and external/error details; use `publication` for one full
record and `attempts` for its attempt history. Use `media` to inspect the
scoped library after an upload response is lost, and follow every relevant
page before deciding what to do next.

Writes are not automatically retried. For an unknown schedule result, retain
the saved ID and body and read `schedule-request`. An `accepted` receipt
identifies the original publications, including their current states. A
`rejected` receipt is terminal and has no publications; inspect its error code
before reviewing a corrected intent. An in-progress response means another
operation still owns the request. A 404 can mean missing or inaccessible and
does not prove an in-flight write failed. Wait, look up the same ID, deliberately
replay the same saved schedule, or explicitly retire that same intent; do not
create a replacement ID on the basis of absence. The server normalizes receipt
timestamps and channel ordering; it compares equivalent normalized values.
Other writes can also have unknown outcomes; inspect their authoritative state.

For known API rejection codes, the CLI and MCP return fixed guidance such as a
text limit or missing posting schedule. They never display remote error messages
or unknown codes, which could contain private data. An authentication error on
a write, like an unrecognized write error, requires checking authoritative state
before retrying; authentication can fail after a change has committed. For list
query errors, use an integer `limit` from 1 to 10. Raw `api GET` requests to
`/posts`, `/media` and `/publications` validate that limit locally before sending.

## MCP configuration

Point an MCP client at the built binary with `mcp` as its only argument. For
example, adapt this client configuration to the installation's actual paths:

```json
{
  "command": "/absolute/path/to/beweft",
  "args": ["mcp"],
  "env": {
    "BEWEFT_URL": "http://127.0.0.1:3000",
    "BEWEFT_TOKEN_FILE": "/absolute/path/to/private-token",
    "BEWEFT_UPLOAD_ROOT": "/absolute/path/to/agent-uploads"
  }
}
```

The server uses newline-delimited JSON-RPC on standard input/output. Tool
metadata distinguishes reads from writes, and errors are returned as MCP tool
errors. The server limits calls per process to 120 per rolling minute by
default; set `BEWEFT_MCP_CALLS_PER_MINUTE` to an integer from 1 to 10000 if the
operator needs another limit. This is not a global limit across processes.

MCP media uploads require `BEWEFT_UPLOAD_ROOT`. The server opens that directory
at startup and permits only regular files underneath it, without following
symlinks in the upload path. Use paths rooted at the configured directory or
its resolved absolute path. Changing the root requires restarting the MCP
process. CLI uploads are explicitly local file operations and are not limited
by this MCP root. Both use the API's upload limit and brand scope. Uploads have
a 16-minute client deadline to allow the API's 15-minute receive window and
response; other HTTP requests retain a two-minute deadline.

CLI JSON input and an MCP frame may each be up to 8 MiB on the wire, including
their respective wrapper. This accommodates compact escaped JSON, including
Unicode escapes, around a valid API payload. API JSON mutation bodies remain
subject to the server's 1 MiB request limit after the CLI serializes them; the
wire allowance does not raise that limit.

## Isolated integration check

`crates/cli/tests/integration.py` starts the built API with fixtures disabled,
creates a uniquely named disposable PostgreSQL database, and exercises CLI and
MCP subprocesses. It does not start a worker or contact providers. By default it
uses the existing `beweft-dev-postgres` container and its local test connection,
matching the server integration harness. Set `BEWEFT_TEST_POSTGRES_CONTAINER`,
`BEWEFT_TEST_DB_USER`, `BEWEFT_TEST_DB_PASSWORD` and `BEWEFT_TEST_DB_PORT` to use
another explicitly isolated test PostgreSQL installation. The database user
needs permission to create and drop test databases. `BEWEFT_TEST_BIN_DIR` selects
another directory containing the binaries.

```sh
cargo build -p beweft-server --bin beweft-server
cargo build -p beweft-cli --bin beweft
python3 crates/cli/tests/integration.py
```

For only the scheduling recovery scenario, set `BEWEFT_CLI_SCHEDULE_ONLY=1`
when running the harness.

The harness removes its own database, temporary files, processes and local HTTP
fixtures when it finishes. It covers scoped rejection, drafts and scheduling,
receipt lookup and same-ID replay after a committed response is lost, retirement,
token-file permissions, redirect
refusal, MCP protocol errors and rate limiting, and a symlink replacement during
an upload. It also checks recovery of a committed upload with a lost response
and complete pagination of 170 large drafts and scoped queue, history and media
records. It does not prove provider access or a self-host installation.

The optional `python3 crates/cli/tests/slow_upload.py` uses a loopback fixture
to hold an upload response for 125 seconds, checking that it outlasts the normal
two-minute request deadline without a retry.
