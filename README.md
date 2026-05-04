# rfc-index

A local cache, search index, and reference graph for IETF RFCs, with a CLI
and an MCP server.

## Why

In my own work it's often useful to ask an LLM to cross-reference code with
regards to various RFCs. Asking this often causes the LLM to perform many
iterations of curl'ing the entire body of the RFC, then running grep or sed
attempting to extract relevant content.

Or worse, the LLM just guesses what the RFC "should" say.

I would rather it do neither of these things.

## Install

```sh
git clone https://github.com/randombit/rfc-index
cd rfc-index
cargo install --path . --bin rfc                       # CLI only
cargo install --path . --bin rfc-mcp                  # MCP server
```

The database lives at `$XDG_DATA_HOME/rfc-index/rfcs.db` by default
(usually `~/.local/share/rfc-index/rfcs.db`). Override with `--db PATH` or
`RFC_INDEX_DB=PATH`.

## Quick start

```sh
rfc index sync         # fetch rfc-index.xml and ingest (~5s, ~10k RFCs)
rfc errata sync        # fetch errata.json (~1s, ~8k errata)

rfc info 9000          # metadata for RFC 9000
rfc info BCP14         # sub-series resolution (also STD3, FYI4, etc.)

rfc get 9000                       # full body, auto-fetched + cached
rfc get 9000 --section 5.2         # just § 5.2 (and its descendants)
rfc get 9000 --sections            # list section numbers + titles

rfc search "connection migration"  # FTS5: phrases, AND/OR/NOT, NEAR(), title:foo
rfc refs 9000                      # outgoing body refs (RFCs 9000 cites)
rfc refs 9000 --to                 # incoming body refs (limited to cached bodies)

rfc errata show 8446 --status verified   # filter by status substring

# Bulk pre-seed for offline use
rfc fetch --title-regex 'TLS|QUIC|PKCS' --since 2018 --xml-only
```

## MCP server

`rfc-mcp` exposes the same surface as MCP tools over stdio, intended for
coding agents (Claude Code, OpenCode, etc.).

Build:

```sh
cargo build --bin rfc-mcp --release
```

Register with Claude Code by adding to `.claude/settings.json` (or your
user-level `~/.claude/settings.json`):

```json
{
  "mcpServers": {
    "rfc-index": {
      "command": "rfc-mcp"
    }
  }
}
```

For OpenCode

```json
  "mcp": {
    "rfc-index": {
      "type": "local",
      "command": [
        "rfc-mcp"
      ],
      "enabled": true
    }
  }
```

Tools exposed: `get_rfc`, `search`, `list_rfcs`, `get_body`, `list_sections`,
`references`, `get_sub_series`, `get_errata`, `sync_index`, `sync_errata`.

The server uses the same database as the CLI, so seeding/querying from either
side is interchangeable. Sync the index/errata via the CLI once (or let the
agent call `sync_index` / `sync_errata` itself) and bodies will then be lazily
fetched and cached as the agent reads them.

## Library

```rust
use rfc_index::{RfcIndex, RfcQuery};

let mut idx = RfcIndex::open_default()?;
idx.sync_index()?;

let r = idx.get(9000)?.unwrap();
println!("{} — {}", r.number(), r.title());

let body = idx.ensure_body(9000)?;
let s = body.section("5.2").unwrap();
println!("{}", s.text);

let hits = idx.search("connection migration", Some(10))?;
for h in hits {
    println!("RFC{} {}", h.number(), h.title());
}
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
