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

Replace example IDs with actual IDs. `draft-create` creates a draft only.
`draft-update` requires the current version. `schedule` requires the post ID,
version, channel IDs and an explicit `intent`: `queue` omits `scheduled_at`
and reserves the next posting-rule slot; `exact` supplies a future timestamp
with an offset. Scheduling can publish when the job is due. The `queue`,
`history`, `post`, `attempts` and `health` commands inspect authoritative
server state. The `api METHOD /path` escape hatch is limited to the same API
origin and bearer scopes.

Writes are not automatically retried. If a request times out or its response is
lost, its outcome can be unknown. Inspect the post, queue, history and attempts
and resolve the outcome before retrying; an immediate absence alone is not proof
that an in-flight request did not commit.

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
by this MCP root. Both use the API's upload limit and brand scope.

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
cargo build -p beweft-server --bin beweft-server -p beweft-cli
python3 crates/cli/tests/integration.py
```

The harness removes its own database, temporary files, processes and local HTTP
fixtures when it finishes. It covers scoped rejection, drafts and scheduling,
readback after a committed response is lost, token-file permissions, redirect
refusal, MCP protocol errors and rate limiting, and a symlink replacement during
an upload. It does not prove provider access or a self-host installation.
