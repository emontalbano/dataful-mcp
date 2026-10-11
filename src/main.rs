use anyhow::Result;
use dataful_mcp::mcp::server::McpServer;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr (stdout is for MCP protocol)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("dataful_mcp=info".parse()?)
                .add_directive("tokio_tungstenite=warn".parse()?),
        )
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    info!("Starting Dataful MCP Server");

    let mut server = McpServer::new();
    server.run().await?;

    Ok(())
}
