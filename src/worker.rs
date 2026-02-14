use crate::AppState;
use crate::eth::submit::submit_settlement;
use crate::models::outbox::SettlementPayload;
use sqlx::Row;


pub async fn run_worker(state: AppState) {
    loop {
        let rows = sqlx::query(
            r#"
            SELECT id, payload, retries
            FROM outbox
            WHERE status = 'PENDING'
            ORDER BY created_at ASC
            LIMIT 10
            "#
        )
        .fetch_all(&state.db)
        .await
        .unwrap();

        for row in rows {
            let job_id: String = row.get("id");
            let payload_str: String = row.get("payload");
            let retries: i64 = row.get("retries"); // SQLite integer -> i64

            let payload: SettlementPayload = match serde_json::from_str(&payload_str) {
                Ok(p) => p,
                Err(e) => {
                    sqlx::query(
                        r#"
                        UPDATE outbox
                        SET status='FAILED',
                            last_error=?,
                            updated_at=DATETIME('now')
                        WHERE id=?
                        "#
                    )
                    .bind(format!("bad payload json: {}", e))
                    .bind(&job_id)
                    .execute(&state.db)
                    .await
                    .unwrap();
                    continue;
                }
            };

            let market_hash_vec = match hex::decode(&payload.market_hash_hex) {
                Ok(v) => v,
                Err(e) => {
                    sqlx::query(
                        r#"
                        UPDATE outbox
                        SET status='FAILED',
                            last_error=?,
                            updated_at=DATETIME('now')
                        WHERE id=?
                        "#
                    )
                    .bind(format!("bad market_hash hex: {}", e))
                    .bind(&job_id)
                    .execute(&state.db)
                    .await
                    .unwrap();
                    continue;
                }
            };

            let leaf_vec = match hex::decode(&payload.leaf_hex) {
                Ok(v) => v,
                Err(e) => {
                    sqlx::query(
                        r#"
                        UPDATE outbox
                        SET status='FAILED',
                            last_error=?,
                            updated_at=DATETIME('now')
                        WHERE id=?
                        "#
                    )
                    .bind(format!("bad leaf hex: {}", e))
                    .bind(&job_id)
                    .execute(&state.db)
                    .await
                    .unwrap();
                    continue;
                }
            };

            if market_hash_vec.len() != 32 || leaf_vec.len() != 32 {
                sqlx::query(
                    r#"
                    UPDATE outbox
                    SET status='FAILED',
                        last_error=?,
                        updated_at=DATETIME('now')
                    WHERE id=?
                    "#
                )
                .bind("hash/leaf wrong length (expected 32 bytes)")
                .bind(&job_id)
                .execute(&state.db)
                .await
                .unwrap();
                continue;
            }

            let mut market_hash = [0u8; 32];
            market_hash.copy_from_slice(&market_hash_vec);

            let mut leaf = [0u8; 32];
            leaf.copy_from_slice(&leaf_vec);

            match submit_settlement(market_hash, leaf, payload.outcome_u64, payload.ts).await {
                Ok(_) => {
                    sqlx::query(
                        r#"
                        UPDATE outbox
                        SET status='SENT',
                            updated_at=DATETIME('now'),
                            last_error=NULL
                        WHERE id=?
                        "#
                    )
                    .bind(&job_id)
                    .execute(&state.db)
                    .await
                    .unwrap();
                }
                Err(e) => {
                    let next_retries = retries + 1;
                    let next_status = if next_retries > 5 { "FAILED" } else { "PENDING" };

                    sqlx::query(
                        r#"
                        UPDATE outbox
                        SET retries=?,
                            last_error=?,
                            status=?,
                            updated_at=DATETIME('now')
                        WHERE id=?
                        "#
                    )
                    .bind(next_retries)
                    .bind(e.to_string())
                    .bind(next_status)
                    .bind(&job_id)
                    .execute(&state.db)
                    .await
                    .unwrap();
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
