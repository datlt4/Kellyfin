use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous};
use std::{path::Path, time::Duration};

pub async fn open(data: &Path) -> anyhow::Result<SqlitePool> {
    std::fs::create_dir_all(data)?;
    let opts = SqliteConnectOptions::new()
        .filename(data.join("kellyfin.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new().max_connections(8).connect_with(opts).await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}
