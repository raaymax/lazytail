//! MCP (Model Context Protocol) server implementation for LazyTail.
//!
//! Provides log file analysis tools accessible via the MCP protocol.
//! Run with `lazytail --mcp` to start the server.

mod tools;
mod types;

use anyhow::Result;
use rmcp::ServiceExt;

/// Run the MCP server using stdio transport.
pub fn run_mcp_server() -> Result<()> {
    // Build tokio runtime for async MCP operations
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async {
        eprintln!("LazyTail MCP server v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("Waiting for MCP client connection...");

        let service = tools::LazyTailMcp::new();
        let running = service.serve(rmcp::transport::stdio()).await?;

        // Wait for the service to complete
        running.waiting().await?;

        Ok(())
    })
}
