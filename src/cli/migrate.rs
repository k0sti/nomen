//! Database migration CLI commands.
//!
//! Migrations are registered as named functions with a version string.
//! Each migration runs once — completed migrations are tracked in the `meta` table.

use anyhow::Result;
use colored::Colorize;
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

use nomen::Nomen;

/// A registered migration.
struct Migration {
    /// Unique version identifier (e.g. "2026-03-24-collected-messages").
    version: &'static str,
    /// Human-readable description.
    description: &'static str,
    /// The migration function. Takes the full Nomen handle for relay access.
    run: fn(&Nomen) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + '_>>,
}

/// All registered migrations, in order.
fn migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: "2026-03-24-raw-to-collected",
            description: "Migrate raw_message records to collected_message (kind 30100)",
            run: |nomen| Box::pin(migrate_raw_to_collected(nomen)),
        },
        Migration {
            version: "2026-03-24-drop-raw-message",
            description: "Drop raw_message table after migration to collected_message",
            run: |nomen| Box::pin(drop_raw_message_table(nomen.db())),
        },
    ]
}

const META_PREFIX: &str = "migration:";

async fn is_migration_done(db: &Surreal<Db>, version: &str) -> Result<bool> {
    let key = format!("{META_PREFIX}{version}");
    let val = nomen_db::get_meta(db, &key).await?;
    Ok(val.is_some())
}

async fn mark_migration_done(db: &Surreal<Db>, version: &str, result: &str) -> Result<()> {
    let key = format!("{META_PREFIX}{version}");
    let now = chrono::Utc::now().to_rfc3339();
    let value = format!("{now}|{result}");
    nomen_db::set_meta(db, &key, &value).await
}

// ── Migration implementations ──────────────────────────────────────

async fn migrate_raw_to_collected(nomen: &Nomen) -> Result<String> {
    let db = nomen.db();

    let raw_messages: Vec<nomen_db::RawMessageRecord> = db
        .query(
            "SELECT meta::id(id) AS id, source, source_id ?? '' AS source_id, sender, \
             channel ?? '' AS channel, content, created_at, consolidated \
             FROM raw_message ORDER BY created_at ASC",
        )
        .await?
        .check()?
        .take(0)?;

    let mut migrated = 0;
    let mut published = 0;
    for msg in &raw_messages {
        // Build a d-tag from source + source_id
        let d_tag = if !msg.source_id.is_empty() {
            format!("{}:{}", msg.source, msg.source_id)
        } else {
            format!("{}:{}:{}", msg.source, msg.created_at, msg.sender)
        };

        // Check if already migrated
        let existing: Option<nomen_db::CollectedMessageRecord> = db
            .query(
                "SELECT d_tag, consolidated FROM collected_message \
                 WHERE d_tag = $dtag LIMIT 1",
            )
            .bind(("dtag", d_tag.clone()))
            .await?
            .check()?
            .take(0)?;

        if existing.is_some() {
            continue;
        }

        // Parse created_at from RFC3339 to unix timestamp
        let created_at = chrono::DateTime::parse_from_rfc3339(&msg.created_at)
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        // Build tags
        let mut tags = vec![
            vec!["d".to_string(), d_tag.clone()],
            vec!["proxy".to_string(), d_tag.clone(), msg.source.clone()],
            vec!["sender".to_string(), msg.sender.clone()],
        ];
        if !msg.channel.is_empty() {
            tags.push(vec!["chat".to_string(), msg.channel.clone()]);
        }

        let author_pubkey = nomen
            .signer()
            .map(|s| s.public_key().to_hex())
            .unwrap_or_default();

        let mut event = nomen_core::collected::CollectedEvent {
            kind: nomen_core::kinds::COLLECTED_MESSAGE_KIND,
            created_at,
            pubkey: author_pubkey,
            tags,
            content: msg.content.clone(),
            id: None,
            sig: None,
        };

        // Publish to relay if available
        if let Some(ref relay) = nomen.relay() {
            let mut nostr_tags: Vec<nostr_sdk::Tag> = Vec::new();
            for tag in &event.tags {
                if tag.len() >= 2 {
                    nostr_tags.push(nostr_sdk::Tag::custom(
                        nostr_sdk::TagKind::Custom(tag[0].clone().into()),
                        tag[1..].to_vec(),
                    ));
                }
            }

            let builder = nostr_sdk::EventBuilder::new(
                nostr_sdk::Kind::Custom(nomen_core::kinds::COLLECTED_MESSAGE_KIND),
                &event.content,
            )
            .tags(nostr_tags);

            match relay.sign_and_publish(builder).await {
                Ok((signed_event, _)) => {
                    event.id = Some(signed_event.id.to_hex());
                    event.sig = Some(signed_event.sig.to_string());
                    event.pubkey = signed_event.pubkey.to_hex();
                    published += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to publish migrated event {d_tag} to relay: {e}");
                }
            }
        }

        // Store via the same DB function used by store_collected_event
        nomen_db::store_collected_event(db, &event).await?;

        migrated += 1;
    }

    let msg = if published > 0 {
        format!("Migrated {migrated} records ({published} published to relay)")
    } else {
        format!("Migrated {migrated} records")
    };
    Ok(msg)
}

async fn drop_raw_message_table(db: &Surreal<Db>) -> Result<String> {
    // Only drop if raw_to_collected migration has been run
    let key = format!("{META_PREFIX}2026-03-24-raw-to-collected");
    let prev = nomen_db::get_meta(db, &key).await?;
    if prev.is_none() {
        anyhow::bail!("Cannot drop raw_message: migration 2026-03-24-raw-to-collected has not been run yet");
    }

    db.query("REMOVE TABLE IF EXISTS raw_message").await?.check()?;
    Ok("Dropped raw_message table".to_string())
}

// ── CLI commands ───────────────────────────────────────────────────

pub async fn cmd_migrate_status(nomen: &Nomen) -> Result<()> {
    let db = nomen.db();
    let all = migrations();

    println!("\n{}\n{}", "Database Migrations".bold(), "═".repeat(50));

    let mut pending = 0;
    for m in &all {
        let done = is_migration_done(db, m.version).await?;
        let status = if done {
            "✅ done".green().to_string()
        } else {
            pending += 1;
            "⬜ pending".yellow().to_string()
        };
        println!("  {} {} — {}", status, m.version.bold(), m.description);
    }

    println!(
        "\n{}: {} total, {} pending\n",
        "Summary".bold(),
        all.len(),
        pending
    );
    Ok(())
}

pub async fn cmd_migrate_run(nomen: &Nomen, dry_run: bool) -> Result<()> {
    let db = nomen.db();
    let all = migrations();

    let mut ran = 0;
    let mut skipped = 0;

    for m in &all {
        if is_migration_done(db, m.version).await? {
            skipped += 1;
            continue;
        }

        if dry_run {
            println!(
                "  {} {} — {}",
                "would run".yellow().bold(),
                m.version,
                m.description
            );
            ran += 1;
            continue;
        }

        print!("  {} {} ... ", "Running".blue().bold(), m.version);
        match (m.run)(nomen).await {
            Ok(result) => {
                println!("{} ({})", "ok".green(), result);
                mark_migration_done(db, m.version, &result).await?;
                ran += 1;
            }
            Err(e) => {
                println!("{} ({})", "FAILED".red().bold(), e);
                println!("\n  Stopping at failed migration. Fix the issue and re-run.");
                return Err(e);
            }
        }
    }

    if ran == 0 && skipped > 0 {
        println!("All migrations already applied ({skipped} skipped).");
    } else if dry_run {
        println!("\n{}: {ran} migrations would run ({skipped} already applied)", "Dry run".yellow().bold());
    } else {
        println!("\n{}: {ran} migrations applied, {skipped} skipped", "Done".green().bold());
    }

    Ok(())
}
