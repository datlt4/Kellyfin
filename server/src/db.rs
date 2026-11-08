use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous};
use std::{path::Path, time::Duration};

pub async fn open(data: &Path) -> anyhow::Result<SqlitePool> {
    std::fs::create_dir_all(data)?;
    #[cfg(unix)]
    {
        // The database holds password hashes and session tokens: keep it and
        // its directory away from other local users (default umask is 022).
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(data, std::fs::Permissions::from_mode(0o700));
    }
    let opts = SqliteConnectOptions::new()
        .filename(data.join("kellyfin.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new().max_connections(8).connect_with(opts).await?;
    sqlx::migrate!().run(&pool).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for f in ["kellyfin.db", "kellyfin.db-wal", "kellyfin.db-shm", "kellyfin.db-journal"] {
            let _ = std::fs::set_permissions(data.join(f), std::fs::Permissions::from_mode(0o600));
        }
    }
    Ok(pool)
}
