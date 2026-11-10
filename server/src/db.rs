use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous};
use std::{path::Path, time::Duration};

/// False when KELLYFIN_STRICT_PERMS is 0/false/no: keeps the data directory
/// and DB readable by other local users (Samba shares, sidecar backups).
/// Default is strict, because the DB holds password hashes and token hashes.
fn strict_perms() -> bool {
    !matches!(std::env::var("KELLYFIN_STRICT_PERMS").unwrap_or_default().to_ascii_lowercase().as_str(), "0" | "false" | "no")
}

pub async fn open(data: &Path) -> anyhow::Result<SqlitePool> {
    std::fs::create_dir_all(data)?;
    #[cfg(unix)]
    if strict_perms() {
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
    if strict_perms() {
        use std::os::unix::fs::PermissionsExt;
        for f in ["kellyfin.db", "kellyfin.db-wal", "kellyfin.db-shm", "kellyfin.db-journal"] {
            let _ = std::fs::set_permissions(data.join(f), std::fs::Permissions::from_mode(0o600));
        }
    }
    Ok(pool)
}
