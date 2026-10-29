//! Integration against actual stored schedule rules and occupied slots, with explicit DST dates.
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
async fn slot(tx: &mut Transaction<'_, Postgres>, channel: Uuid, now: &str) -> DateTime<Utc> {
    let rules =
        sqlx::query("SELECT days,local_time,timezone FROM schedule_rules WHERE channel_id=$1")
            .bind(channel)
            .fetch_all(&mut **tx)
            .await
            .unwrap();
    beweft::schedules::next_slot_after(tx, channel, rules, now.parse().unwrap())
        .await
        .unwrap()
}
#[tokio::test]
#[ignore = "requires BEWEFT_TEST_DATABASE_URL; the Python integration runner supplies an isolated migrated database"]
async fn daylight_saving_and_occupied_slots() {
    let pool = PgPool::connect(
        &std::env::var("BEWEFT_TEST_DATABASE_URL").expect("isolated test database"),
    )
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let brand = Uuid::new_v4();
    let channel = Uuid::new_v4();
    let rule = Uuid::new_v4();
    sqlx::query("INSERT INTO brands(id,name) VALUES($1,'DST integration')")
        .bind(brand)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO channels(id,brand_id,name,provider) VALUES($1,$2,'DST channel','manual')",
    )
    .bind(channel)
    .bind(brand)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO schedule_rules(id,channel_id,days,local_time,timezone) VALUES($1,$2,'[7]','02:30','America/Chicago')").bind(rule).bind(channel).execute(&mut *tx).await.unwrap();
    // Spring-forward local 02:30 does not exist. Skip that day rather than silently changing the local time.
    assert_eq!(
        slot(&mut tx, channel, "2026-03-08T06:00:00Z")
            .await
            .to_rfc3339(),
        "2026-03-15T07:30:00+00:00"
    );
    sqlx::query("UPDATE schedule_rules SET local_time='01:30' WHERE id=$1")
        .bind(rule)
        .execute(&mut *tx)
        .await
        .unwrap();
    // Repeated autumn time receives exactly one slot, the earlier offset.
    let autumn = slot(&mut tx, channel, "2026-11-01T05:00:00Z").await;
    assert_eq!(autumn.to_rfc3339(), "2026-11-01T06:30:00+00:00");
    assert_eq!(
        slot(&mut tx, channel, "2026-11-01T06:45:00Z")
            .await
            .to_rfc3339(),
        "2026-11-08T07:30:00+00:00"
    );
    let post = Uuid::new_v4();
    sqlx::query("INSERT INTO posts(id,brand_id,text) VALUES($1,$2,'Occupied DST slot')")
        .bind(post)
        .bind(brand)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload,scheduling_mode) VALUES($1,$2,$3,$4,1,$5,'queued',$6,'queue')").bind(Uuid::new_v4()).bind(post).bind(brand).bind(channel).bind(autumn).bind(json!({"text":"Occupied DST slot","title":"","media_ids":[],"options":{}})).execute(&mut *tx).await.unwrap();
    assert_eq!(
        slot(&mut tx, channel, "2026-11-01T05:00:00Z")
            .await
            .to_rfc3339(),
        "2026-11-08T07:30:00+00:00"
    );
    tx.rollback().await.unwrap();
    pool.close().await;
}
