# ADR-010: MCP Server Integration

## Status

Accepted

## Context

AI assistants (like Claude Code) can benefit from programmatic access to log files for debugging and analysis. The Model Context Protocol (MCP) provides a standard interface for tools that AI assistants can use.

Options considered:
1. **REST API** - HTTP server with JSON endpoints
2. **MCP server** - stdio-based MCP protocol using rmcp
3. **CLI subcommands** - `lazytail search`, `lazytail tail`, etc. (AI calls via shell)
4. **No programmatic interface** - TUI only

## Decision

We implement an **MCP server** as an optional, feature-gated component:

```toml
[features]
default = ["mcp"]
mcp = ["dep:tokio", "dep:rmcp", "dep:schemars"]
```

Activated via `lazytail --mcp`, it runs a stdio transport MCP server providing 6 tools:

| Tool | Purpose |
|------|---------|
| `list_sources` | Discover available log sources with status and metadata |
| `search` | Find patterns (plain text, regex, or structured queries) |
| `get_lines` | Read specific line ranges from a source |
| `get_tail` | Fetch the most recent N lines |
| `get_context` | Get lines around a specific line number |
| `get_stats` | Index metadata and severity breakdown |

The MCP server reuses the same source discovery, config, and query parsing code as the TUI. It uses the same `DiscoveryResult` context to find sources in both project-local and global directories.

ANSI escape codes are stripped from MCP output since AI assistants consume plain text.

## Consequences

**Benefits:**
- AI assistants can search, read, and analyze logs programmatically
- Reuses existing source discovery and query infrastructure
- Feature-gated: users who don't need MCP pay no compilation cost
- stdio transport: no port management, works in any environment

**Trade-offs:**
- Adds tokio, rmcp, and schemars as optional dependencies
- MCP server runs as a separate process (not integrated into the TUI)
- Structured queries use JSON format for MCP but text format for TUI (two parsers, shared AST)
- Must strip ANSI codes for clean MCP output
