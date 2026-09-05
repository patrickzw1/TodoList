use anyhow::{Context, Result};
use rmcp::{transport::stdio, ServiceExt};
use std::{env, fs, path::PathBuf};
use task_store_sqlite::SqliteTaskStore;
use todolist_mcp::TodoMcpServer;

fn database_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("TODOLIST_DB_PATH") {
        return Ok(PathBuf::from(path));
    }
    let data_directory =
        dirs::data_dir().context("could not locate the application data directory")?;
    Ok(data_directory
        .join("app.todolist.desktop")
        .join("todolist.sqlite"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let database_path = database_path()?;
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
