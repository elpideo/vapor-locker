use std::time::Duration;

use anyhow::Context;
use sqlx::{postgres::PgPoolOptions, PgPool};

#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
struct FoundRow {
    id: i64,
    value: String,
    ephemeral: bool,
}

impl Db {
    pub async fn connect_from_env() -> anyhow::Result<Self> {
        let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL missing")?;
        let max_connections: u32 = std::env::var("DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&database_url)
            .await
            .context("connect postgres")?;

        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .context("run migrations")?;
        Ok(())
    }

    pub async fn insert(&self, key: &str, value: &str, ephemeral: bool) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO entries (key, value, ephemeral)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(key)
        .bind(value)
        .bind(ephemeral)
        .execute(&self.pool)
        .await
        .context("insert entry")?;
        Ok(())
    }

    /// Get the most recent, non-expired entry for a key.
    /// If it is ephemeral, it is deleted in the same transaction.
    pub async fn get_value_maybe_delete_ephemeral(
        &self,
        key: &str,
    ) -> anyhow::Result<Option<String>> {
        let mut tx = self.pool.begin().await.context("begin tx")?;

        let row: Option<FoundRow> = sqlx::query_as(
            r#"
            SELECT id, value, ephemeral
            FROM entries
            WHERE key = $1
              AND created_at >= (now() - interval '24 hours')
            ORDER BY created_at DESC
            LIMIT 1
            FOR UPDATE
            "#,
        )
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .context("select entry")?;

        let Some(row) = row else {
            tx.commit().await.ok();
            return Ok(None);
        };

        if row.ephemeral {
            sqlx::query("DELETE FROM entries WHERE id = $1")
                .bind(row.id)
                .execute(&mut *tx)
                .await
                .context("delete ephemeral")?;
        }

        tx.commit().await.context("commit tx")?;
        Ok(Some(row.value))
    }

    pub async fn purge_expired(&self) -> anyhow::Result<u64> {
        let res = sqlx::query(
            r#"
            DELETE FROM entries
            WHERE created_at < (now() - interval '24 hours')
            "#,
        )
        .execute(&self.pool)
        .await
        .context("purge expired")?;
        Ok(res.rows_affected())
    }
}

