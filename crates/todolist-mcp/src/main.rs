use anyhow::{Context, Result};
use rmcp::{transport::stdio, ServiceExt};
use std::fs;
use task_store_sqlite::{storage_path::database_path, SqliteTaskStore};
use todolist_mcp::TodoMcpServer;

#[tokio::main]
async fn main() -> Result<()> {
    let database_path = database_path().map_err(anyhow::Error::msg)?;
    if std::env::args().nth(1).as_deref() == Some("--print-storage-path") {
        println!("{}", database_path.display());
        return Ok(());
    }
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent).context("could not create TodoList data directory")?;
    }
    let store = SqliteTaskStore::open(&database_path)
        .map_err(anyhow::Error::msg)
        .context("could not open TodoList database")?;
    let service = TodoMcpServer::new(store).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
