pub mod dao;
pub mod schema;

use rusqlite::Connection;

use crate::error::AppResult;

/// Initialize the database schema.
/// Called once at application startup.
/// Uses `CREATE TABLE IF NOT EXISTS` so it's safe to call repeatedly.
pub fn init_db(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(schema::CREATE_TABLES)?;
    log::info!("Database schema initialized");
    Ok(())
}
