use anyhow::{Context, Result};
use rmcp::{transport::stdio, ServiceExt};
use std::fs;
use task_diagnostics::{Component, DiagnosticLog, PathIdentity, Record};
use task_store_sqlite::{
    storage_path::{database_path, StorageClient},
    SqliteTaskStore,
};
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
    let print_storage_path = std::env::args().nth(1).as_deref() == Some("--print-storage-path");
    if print_storage_path {
        let database_path = database_path(StorageClient::Mcp).map_err(anyhow::Error::msg)?;
        println!("{}", database_path.display());
        return Ok(());
    }
    let logger = DiagnosticLog::for_current_exe(Component::Mcp);
    if let Some(reason) = logger.status().reason {
        eprintln!("TodoList MCP logs unavailable: {reason}");
    }
    let channel = if cfg!(feature = "production") {
        "production"
    } else {
        "development"
    };
    logger
        .record(Record::new("startup", "mcp", "started").build(env!("CARGO_PKG_VERSION"), channel));
    let database_path = database_path(StorageClient::Mcp).map_err(|error| {
        logger.record(Record::new("startup_failed", "database_path", "error").error(&error));
        anyhow::Error::msg(error)
    })?;
    logger.record(Record::new("database_selected", "startup", "ok").source(
        PathIdentity::database(
            &database_path,
            std::env::var_os("TODOLIST_DB_PATH").is_some(),
        ),
    ));
    if install_lock_path()?.is_file() {
        logger.record(Record::new("startup_blocked", "installer", "error"));
        anyhow::bail!(
            "TodoList is being updated; retry this MCP request after installation finishes"
        );
    }
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent)
            .inspect_err(|error| {
                logger.record(
                    Record::new("startup_failed", "data_directory", "error")
                        .error(&error.to_string()),
                )
            })
            .context("could not create TodoList data directory")?;
    }
    let store = SqliteTaskStore::open(&database_path)
        .map_err(|error| {
            logger.record(Record::new("startup_failed", "database_open", "error").error(&error));
            anyhow::Error::msg(error)
        })
        .context("could not open TodoList database")?;
    logger.record(Record::new("startup", "mcp", "ready"));
    let service = TodoMcpServer::with_logger(store, logger.clone())
        .serve(stdio())
        .await
        .inspect_err(|error| {
            logger
                .record(Record::new("transport_failed", "stdio", "error").error(&error.to_string()))
        })?;
    service.waiting().await.inspect_err(|error| {
        logger.record(Record::new("transport_failed", "stdio", "error").error(&error.to_string()))
    })?;
    logger.record(Record::new("shutdown", "mcp", "ok"));
    Ok(())
}
