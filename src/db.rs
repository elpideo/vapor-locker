use std::time::Duration;

use anyhow::Context;
use rand::RngCore;
use sqlx::{postgres::PgPoolOptions, PgPool};
use time::OffsetDateTime;

/// Accès PostgreSQL (pool `sqlx`) et opérations applicatives (entries, salts, purge).
#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

/// Entrée trouvée en DB (avec métadonnées pour TTL / éphémère).
#[derive(Debug, Clone)]
pub struct FoundEntry {
    pub value: String,
    pub ephemeral: bool,
    pub created_at: OffsetDateTime,
}

/// Ligne matérialisée lors d’un `SELECT` d’entrée (avec id pour suppression éventuelle).
#[derive(Debug, sqlx::FromRow)]
struct FoundRow {
    id: i64,
    value: String,
    ephemeral: bool,
    created_at: OffsetDateTime,
}

/// Ligne matérialisée lors d’un `SELECT` de sels.
#[derive(Debug, sqlx::FromRow)]
struct SaltRow {
    salt: Vec<u8>,
    created_at: OffsetDateTime,
}

/// Statistiques de purge retournées à la CLI (serveur / purge).
#[derive(Debug, Clone, Copy)]
pub struct PurgeStats {
    pub entries_deleted: u64,
    pub salts_deleted: u64,
}

impl Db {
    /// Connecte la base depuis les variables d’environnement.
    ///
    /// - `DATABASE_URL` (requis)
    /// - `DB_MAX_CONNECTIONS` (optionnel, défaut 10)
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

    /// Exécute les migrations `sqlx` depuis `./migrations`.
    pub async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .context("run migrations")?;
        Ok(())
    }

    /// Insère une nouvelle entrée.
    ///
    /// `value` est stockée telle quelle (payload chiffré sérialisé en JSON).
    pub async fn insert(&self, key_hash: &str, value: &str, ephemeral: bool) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO entries (key_hash, value, ephemeral)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(key_hash)
        .bind(value)
        .bind(ephemeral)
        .execute(&self.pool)
        .await
        .context("insert entry")?;
        Ok(())
    }

    /// Get the most recent, non-expired entry for any key_hash in the list.
    /// If it is ephemeral, it is deleted in the same transaction.
    pub async fn get_value_by_hashes_maybe_delete_ephemeral(
        &self,
        key_hashes: Vec<String>,
    ) -> anyhow::Result<Option<FoundEntry>> {
        if key_hashes.is_empty() {
            return Ok(None);
        }

        let mut tx = self.pool.begin().await.context("begin tx")?;

        let row: Option<FoundRow> = sqlx::query_as(
            r#"
            SELECT id, value, ephemeral, created_at
            FROM entries
            WHERE key_hash = ANY($1)
              AND created_at >= (now() - interval '24 hours')
            ORDER BY created_at DESC
            LIMIT 1
            FOR UPDATE
            "#,
        )
        .bind(key_hashes)
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
        Ok(Some(FoundEntry {
            value: row.value,
            ephemeral: row.ephemeral,
            created_at: row.created_at,
        }))
    }

    /// Retourne les sels valides et en crée un nouveau si nécessaire (rotation ~1h).
    ///
    /// Les sels sont considérés valides pendant ~25h afin de couvrir une fenêtre de dérivation côté client.
    pub async fn list_valid_salts_with_rotation(&self) -> anyhow::Result<Vec<Vec<u8>>> {
        // Ensure there's a recent salt (rotation ~1h).
        let now = OffsetDateTime::now_utc();
        let latest: Option<OffsetDateTime> = sqlx::query_scalar(
            r#"
            SELECT created_at
            FROM salts
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .context("select latest salt created_at")?;

        let need_new = match latest {
            None => true,
            Some(ts) => ts < (now - time::Duration::hours(1)),
        };

        if need_new {
            let mut bytes = [0u8; 16]; // 128-bit salt
            rand::thread_rng().fill_bytes(&mut bytes);
            sqlx::query(
                r#"
                INSERT INTO salts (salt)
                VALUES ($1)
                "#,
            )
            .bind(bytes.as_slice())
            .execute(&self.pool)
            .await
            .context("insert salt")?;
        }

        let rows: Vec<SaltRow> = sqlx::query_as(
            r#"
            SELECT salt, created_at
            FROM salts
            WHERE created_at >= (now() - interval '25 hours')
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .context("select valid salts")?;

        Ok(rows.into_iter().map(|r| r.salt).collect())
    }

    /// Purge les entrées expirées (24h) et les sels expirés (25h).
    pub async fn purge_expired(&self) -> anyhow::Result<PurgeStats> {
        let entries_res = sqlx::query(
            r#"
            DELETE FROM entries
            WHERE created_at < (now() - interval '24 hours')
            "#,
        )
        .execute(&self.pool)
        .await
        .context("purge expired entries")?;

        let salts_res = sqlx::query(
            r#"
            DELETE FROM salts
            WHERE created_at < (now() - interval '25 hours')
            "#,
        )
        .execute(&self.pool)
        .await
        .context("purge expired salts")?;

        Ok(PurgeStats {
            entries_deleted: entries_res.rows_affected(),
            salts_deleted: salts_res.rows_affected(),
        })
    }
}

