use anyhow::{Context, Result};
use rmcp::{transport::stdio, ServiceExt};
use std::fs;
use task_store_sqlite::{storage_path::database_path, SqliteTaskStore};
use todolist_mcp::TodoMcpServer;

fn build_identity() -> String {
    let channel = if cfg!(feature = "production") {
        "production"
    } else {
        "development"
    };
    format!("todolist/{}/{channel}", env!("CARGO_PKG_VERSION"))
}

fn install_lock_path() -> Result<std::path::PathBuf> {
    let executable =
        std::env::current_exe().context("could not resolve TodoList MCP executable")?;
    let directory = executable
        .parent()
        .context("TodoList MCP executable has no parent directory")?;
    Ok(directory.join(".todolist-installing.json"))
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--print-build-identity") {
        println!("{}", build_identity());
        return Ok(());
    }
    let database_path = database_path().map_err(anyhow::Error::msg)?;
    if std::env::args().nth(1).as_deref() == Some("--print-storage-path") {
        println!("{}", database_path.display());
        return Ok(());
    }
    if install_lock_path()?.is_file() {
        anyhow::bail!(
            "TodoList is being updated; retry this MCP request after installation finishes"
        );
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
