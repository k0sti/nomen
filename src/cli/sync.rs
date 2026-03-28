//! Sync-related CLI commands: sync, embed, consolidate, cluster, entities.

use anyhow::Result;
use colored::Colorize;
use serde_json::json;

use nomen::Nomen;

use super::helpers::{cli_dispatch, Backend};

pub async fn cmd_sync(backend: &Backend, nomen: Option<&Nomen>) -> Result<()> {
    println!("Connecting to relay...");
    let result = cli_dispatch(backend, nomen, "memory.sync", &json!({})).await?;
    let stored = result["stored"].as_u64().unwrap_or(0);
    let skipped = result["skipped"].as_u64().unwrap_or(0);
    let errors = result["errors"].as_u64().unwrap_or(0);
    println!(
        "Sync complete: {} stored, {} skipped (already up to date)",
        stored.to_string().green(),
        skipped
    );
    if errors > 0 {
        println!("  {} errors during sync", errors.to_string().red());
    }
    Ok(())
}

pub async fn cmd_embed(backend: &Backend, nomen: Option<&Nomen>, limit: usize) -> Result<()> {
    let result = cli_dispatch(backend, nomen, "memory.embed", &json!({"limit": limit})).await?;
    let total = result["total"].as_u64().unwrap_or(0);
    let embedded = result["embedded"].as_u64().unwrap_or(0);

    if total == 0 {
        println!("All memories already have embeddings.");
    } else {
        println!("{}: {} memories embedded", "Done".green().bold(), embedded);
    }

    Ok(())
}

pub async fn cmd_consolidate(
    backend: &Backend,
    nomen: Option<&Nomen>,
    min_messages: usize,
    batch_size: usize,
    dry_run: bool,
    older_than: Option<String>,
    tier: Option<String>,
) -> Result<()> {
    if dry_run {
        println!(
            "{} Running consolidation pipeline...",
            "[DRY RUN]".yellow().bold()
        );
    } else {
        println!("Running consolidation pipeline...");
    }

    let params = json!({
        "min_messages": min_messages,
        "batch_size": batch_size,
        "dry_run": dry_run,
        "older_than": older_than,
        "tier": tier,
    });

    let result = cli_dispatch(backend, nomen, "memory.consolidate", &params).await?;

    let memories_created = result["memories_created"].as_u64().unwrap_or(0);
    let messages_processed = result["messages_processed"].as_u64().unwrap_or(0);
    let events_published = result["events_published"].as_u64().unwrap_or(0);
    let events_deleted = result["events_deleted"].as_u64().unwrap_or(0);

    if memories_created == 0 {
        println!("Nothing to consolidate (need at least {min_messages} unconsolidated messages).");
    } else {
        let prefix = if dry_run {
            format!("{}", "[DRY RUN] Would consolidate".yellow())
        } else {
            format!("{}", "Consolidated".green().bold())
        };
        println!(
            "{}: {} messages → {} memories",
            prefix, messages_processed, memories_created
        );
        if events_published > 0 {
            println!(
                "  Published {} memories to relay (kind 31234)",
                events_published
            );
        }
        if events_deleted > 0 {
            println!(
                "  Deleted {} ephemeral events from relay (NIP-09)",
                events_deleted
            );
        }
        if let Some(channels) = result["channels"].as_array() {
            let channel_strs: Vec<&str> = channels.iter().filter_map(|c| c.as_str()).collect();
            if !channel_strs.is_empty() {
                println!("  Channels: {}", channel_strs.join(", "));
            }
        }
    }

    Ok(())
}

pub async fn cmd_entities(
    backend: &Backend,
    nomen: Option<&Nomen>,
    kind_filter: Option<&str>,
    show_relations: bool,
) -> Result<()> {
    let params = json!({ "kind": kind_filter });
    let result = cli_dispatch(backend, nomen, "entity.list", &params).await?;
    let entities = result["entities"].as_array();

    if entities.map(|e| e.is_empty()).unwrap_or(true) {
        println!("No entities found.");
        return Ok(());
    }
    let entities = entities.unwrap();

    println!("\n{}\n{}", "Entities".bold(), "═".repeat(60));

    for entity in entities {
        let name = entity["name"].as_str().unwrap_or("");
        let kind = entity["kind"].as_str().unwrap_or("");
        let topic = entity["topic"].as_str().unwrap_or("");
        let created_at = entity["created_at"].as_str().unwrap_or("");
        println!("\n  {} [{}]", name.bold(), kind.yellow());
        println!("    Topic: {}  Created: {}", topic.dimmed(), created_at.dimmed());
    }

    println!("\n{}: {} entities", "Total".bold(), entities.len());

    if show_relations {
        let rel_result = cli_dispatch(backend, nomen, "entity.relationships", &json!({})).await?;
        let relationships = rel_result["relationships"].as_array();

        if relationships.map(|r| r.is_empty()).unwrap_or(true) {
            println!("\nNo relationships found.");
        } else {
            let relationships = relationships.unwrap();
            println!("\n{}\n{}", "Relationships".bold(), "═".repeat(60));
            for rel in relationships {
                let relation = rel["relation"].as_str().unwrap_or("");
                let topic = rel["topic"].as_str().unwrap_or("");
                let content = rel["content"].as_str().unwrap_or("");
                println!(
                    "  {} → {} ({})",
                    relation.bold(),
                    topic,
                    content.dimmed(),
                );
            }
            println!(
                "\n{}: {} relationships",
                "Total".bold(),
                relationships.len()
            );
        }
    }

    println!();
    Ok(())
}

pub async fn cmd_cluster(
    backend: &Backend,
    nomen: Option<&Nomen>,
    dry_run: bool,
    prefix: Option<String>,
    min_members: usize,
    namespace_depth: usize,
) -> Result<()> {
    if dry_run {
        println!("{} Running cluster fusion...", "[DRY RUN]".yellow().bold());
    } else {
        println!("Running cluster fusion...");
    }

    let params = json!({
        "prefix": prefix,
        "min_members": min_members,
        "namespace_depth": namespace_depth,
        "dry_run": dry_run,
    });

    let result = cli_dispatch(backend, nomen, "memory.cluster", &params).await?;

    let clusters_found = result["clusters_found"].as_u64().unwrap_or(0);
    let clusters_synthesized = result["clusters_synthesized"].as_u64().unwrap_or(0);
    let memories_scanned = result["memories_scanned"].as_u64().unwrap_or(0);
    let edges_created = result["edges_created"].as_u64().unwrap_or(0);

    if clusters_found == 0 {
        println!("No clusters found (need at least {min_members} memories per namespace prefix).");
        if memories_scanned == 0 {
            println!("  No named memories in the database. Run `nomen consolidate` first.");
        } else {
            println!(
                "  Scanned {} memories at namespace depth {}.",
                memories_scanned, namespace_depth
            );
        }
    } else {
        let prefix_display = if dry_run {
            format!("{}", "[DRY RUN] Would synthesize".yellow())
        } else {
            format!("{}", "Synthesized".green().bold())
        };

        println!(
            "{}: {} clusters from {} memories",
            prefix_display,
            if dry_run {
                clusters_found
            } else {
                clusters_synthesized
            },
            memories_scanned
        );

        if !dry_run && edges_created > 0 {
            println!("  Created {} 'summarizes' edges", edges_created);
        }

        if let Some(details) = result["cluster_details"].as_array() {
            for detail in details {
                let pfx = detail["prefix"].as_str().unwrap_or("");
                let member_count = detail["member_count"].as_u64().unwrap_or(0);
                println!("\n  {} ({} members)", pfx.bold(), member_count);
                if let Some(topics) = detail["member_topics"].as_array() {
                    for topic in topics {
                        let t = topic.as_str().unwrap_or("");
                        println!("    - {}", t.dimmed());
                    }
                }
            }
        }
    }

    println!();
    Ok(())
}
