use anyhow::Result;
use sqlx::{Row, SqlitePool};

pub struct ChunkRow {
    pub source_path: String,
    pub text: String,
    pub vec: Vec<u8>,
}

/// Replace all chunks for a source with `chunks` (text, hash, vector).
pub async fn replace_source(
    pool: &SqlitePool,
    scope: &str,
    scope_id: &str,
    source_kind: &str,
    source_id: Option<&str>,
    source_path: &str,
    chunks: &[(String, String, Vec<f32>)],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    if let Some(sid) = source_id {
        sqlx::query("DELETE FROM embeddings WHERE source_kind = ?1 AND source_id = ?2")
            .bind(source_kind)
            .bind(sid)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query(
            "DELETE FROM embeddings WHERE scope = ?1 AND scope_id = ?2 AND source_kind = ?3 AND source_path = ?4",
        )
        .bind(scope)
        .bind(scope_id)
        .bind(source_kind)
        .bind(source_path)
        .execute(&mut *tx)
        .await?;
    }
    let now = chrono::Utc::now().timestamp_millis();
    for (i, (text, hash, vec)) in chunks.iter().enumerate() {
        let id = uuid::Uuid::new_v4().to_string();
        let bytes = encode_vec(vec);
        sqlx::query(
            "INSERT INTO embeddings \
             (id, scope, scope_id, source_path, source_kind, source_id, chunk_index, text, hash, dims, vec, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        )
        .bind(&id)
        .bind(scope)
        .bind(scope_id)
        .bind(source_path)
        .bind(source_kind)
        .bind(source_id)
        .bind(i as i64)
        .bind(text)
        .bind(hash)
        .bind(vec.len() as i64)
        .bind(bytes)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn delete_scope(pool: &SqlitePool, scope: &str, scope_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM embeddings WHERE scope = ?1 AND scope_id = ?2")
        .bind(scope)
        .bind(scope_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_source(pool: &SqlitePool, source_kind: &str, source_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM embeddings WHERE source_kind = ?1 AND source_id = ?2")
        .bind(source_kind)
        .bind(source_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete project-scoped chunks plus all chunks of the project's chats. Must
/// run BEFORE the project (and its chats via cascade) are deleted.
pub async fn delete_project_and_chats(pool: &SqlitePool, project_id: &str) -> Result<()> {
    sqlx::query(
        "DELETE FROM embeddings WHERE (scope = 'project' AND scope_id = ?1) \
         OR (scope = 'chat' AND scope_id IN (SELECT id FROM chats WHERE project_id = ?1))",
    )
    .bind(project_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_all(pool: &SqlitePool) -> Result<()> {
    sqlx::query("DELETE FROM embeddings").execute(pool).await?;
    Ok(())
}

/// Remove attachment chunks whose attachment row no longer exists.
pub async fn delete_orphan_attachments(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "DELETE FROM embeddings WHERE source_kind = 'attachment' \
         AND source_id NOT IN (SELECT id FROM attachments)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn count_for_scopes(pool: &SqlitePool, scopes: &[(&str, &str)]) -> Result<i64> {
    if scopes.is_empty() {
        return Ok(0);
    }
    let mut qb =
        sqlx::QueryBuilder::<sqlx::Sqlite>::new("SELECT COUNT(*) as c FROM embeddings WHERE ");
    let mut sep = qb.separated(" OR ");
    for (sc, sid) in scopes {
        sep.push("(scope = ")
            .push_bind(*sc)
            .push(" AND scope_id = ")
            .push_bind(*sid)
            .push(")");
    }
    let row = qb.build().fetch_one(pool).await?;
    let c: i64 = row.try_get("c")?;
    Ok(c)
}

pub async fn count_all(pool: &SqlitePool) -> Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) as c FROM embeddings")
        .fetch_one(pool)
        .await?;
    let c: i64 = row.try_get("c")?;
    Ok(c)
}

pub async fn list_for_scopes(pool: &SqlitePool, scopes: &[(&str, &str)]) -> Result<Vec<ChunkRow>> {
    if scopes.is_empty() {
        return Ok(vec![]);
    }
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, source_path, text, vec FROM embeddings WHERE ",
    );
    let mut sep = qb.separated(" OR ");
    for (sc, sid) in scopes {
        sep.push("(scope = ")
            .push_bind(*sc)
            .push(" AND scope_id = ")
            .push_bind(*sid)
            .push(")");
    }
    let rows = qb.build().fetch_all(pool).await?;
    rows.iter().map(chunk_from_row).collect()
}

fn chunk_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ChunkRow> {
    Ok(ChunkRow {
        source_path: row.try_get("source_path")?,
        text: row.try_get("text")?,
        vec: row.try_get("vec")?,
    })
}

pub fn encode_vec(v: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for f in v {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

pub fn decode_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// Top-K chunks by cosine similarity to `query`. Returns (row index, score).
/// Vectors whose dimension differs from the query are scored 0.
pub fn top_k(query: &[f32], rows: &[ChunkRow], k: usize) -> Vec<(usize, f32)> {
    let qn = norm(query);
    if qn == 0.0 || rows.is_empty() {
        return vec![];
    }
    let mut scored: Vec<(usize, f32)> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let v = decode_vec(&r.vec);
            let n = norm(&v);
            let sim = if n == 0.0 || v.len() != query.len() {
                0.0
            } else {
                dot(query, &v) / (qn * n)
            };
            (i, sim)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(k).collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn norm(a: &[f32]) -> f32 {
    dot(a, a).sqrt()
}
