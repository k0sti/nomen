//! Memory-related CLI commands: store, delete, search, list.

use anyhow::{bail, Result};
use colored::Colorize;
use nostr_sdk::prelude::{Kind, ToBech32};
use serde_json::json;
use tracing::debug;

use nomen::db;
use nomen::display::display_memories;
use nomen::kinds::{LEGACY_LESSON_KIND, LESSON_KIND};
use nomen::memory::{get_tag_value, parse_event};
use nomen::Nomen;

use super::helpers::{build_relay_manager, build_signer, cli_dispatch, parse_keys, Backend};

pub async fn cmd_list_relay(relay_url: &str, nsecs: &[String], named: bool) -> Result<()> {
    let (all_keys, pubkeys) = parse_keys(nsecs)?;
    debug!("Parsed {} keys", all_keys.len());

    let signer = build_signer(&all_keys[0]);
    let mgr = build_relay_manager(relay_url, &all_keys[0]);
    mgr.connect().await?;
    let events = mgr.fetch_memories(&pubkeys).await?;

    let mut memories = Vec::new();
    let mut lesson_count = 0usize;

    for event in events.into_iter() {
        if event.kind == Kind::Custom(LESSON_KIND) || event.kind == Kind::Custom(LEGACY_LESSON_KIND)
        {
            lesson_count += 1;
            continue;
        }
        let d_tag = get_tag_value(&event.tags, "d").unwrap_or_default();
        if d_tag.starts_with("snowclaw:config:") {
            continue;
        }

        if named {
            let topic = nomen::memory::parse_d_tag(&d_tag);
            if topic.starts_with("conv:") || topic.starts_with("consolidated/") {
                continue;
            }
        }

        memories.push(parse_event(&event, signer.as_ref()));
    }

    let npubs: Vec<String> = all_keys
        .iter()
        .filter_map(|k| k.public_key().to_bech32().ok())
        .collect();

    display_memories(&npubs, &memories, lesson_count);
    mgr.disconnect().await;
    Ok(())
}

pub async fn cmd_list_local(
    backend: &Backend,
    nomen: Option<&Nomen>,
    ephemeral: bool,
    stats: bool,
) -> Result<()> {
    if stats {
        let result = cli_dispatch(
            backend,
            nomen,
            "memory.list",
            &json!({"stats": true, "limit": 0}),
        )
        .await?;
        if let Some(s) = result.get("stats") {
            let total = s["total"].as_u64().unwrap_or(0);
            let pending = s["pending"].as_u64().unwrap_or(0);
            println!("\n{}\n{}", "Memory Statistics".bold(), "═".repeat(40));
            println!("  Named memories: {}", total);
            println!("  Ephemeral (pending): {}", pending.to_string().yellow());
            println!();
        }
        return Ok(());
    }

    if ephemeral {
        if matches!(backend, Backend::Http(..)) {
            bail!("This command requires direct DB access. Stop the nomen service first.");
        }
        let db_handle = db::init_db().await?;
        let messages = db::get_unconsolidated_collected(&db_handle, 200, None, None).await?;
        if messages.is_empty() {
            println!("No ephemeral messages pending consolidation.");
            return Ok(());
        }
        println!(
            "\n{} ({} pending)\n{}",
            "Ephemeral Messages".bold(),
            messages.len(),
            "═".repeat(60)
        );
        for msg in &messages {
            let platform = msg.platform.as_deref().unwrap_or("unknown");
            let sender = msg.sender_id.as_deref().unwrap_or("unknown");
            let chat = msg.chat_id.as_deref().unwrap_or("");
            let channel_display = if chat.is_empty() {
                String::new()
            } else {
                format!(" #{chat}")
            };
            println!(
                "  [{}] {}{}: {}",
                platform,
                sender.bold(),
                channel_display,
                if msg.content.len() > 80 {
                    format!("{}...", &msg.content[..80])
                } else {
                    msg.content.clone()
                }
            );
            let ts = chrono::DateTime::from_timestamp(msg.created_at, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();
            println!("    {}", ts.dimmed());
        }
        println!("\n{}: {} messages\n", "Total".bold(), messages.len());
    }

    Ok(())
}

pub async fn cmd_store(
    backend: &Backend,
    nomen: Option<&Nomen>,
    topic: &str,
    content: &str,
    tier: &str,
) -> Result<()> {
    println!("Publishing to relay...");

    let params = json!({
        "topic": topic,
        "content": content,
        "visibility": tier,
    });

    let result = cli_dispatch(backend, nomen, "memory.put", &params).await?;
    let d_tag = result["d_tag"].as_str().unwrap_or("");

    println!(
        "{} stored: {} [{}]",
        "Memory".green().bold(),
        topic.bold(),
        tier
    );
    println!("  d_tag: {}", d_tag);

    Ok(())
}

pub async fn cmd_delete(
    backend: &Backend,
    nomen: Option<&Nomen>,
    topic: Option<&str>,
    event_id: Option<&str>,
) -> Result<()> {
    if topic.is_none() && event_id.is_none() {
        bail!("Provide either a topic or --id <event-id>");
    }

    let mut params = json!({});
    if let Some(topic) = topic {
        params["topic"] = json!(topic);
    }
    if let Some(id) = event_id {
        params["id"] = json!(id);
    }

    cli_dispatch(backend, nomen, "memory.delete", &params).await?;

    if let Some(topic) = topic {
        println!("{} Memory with topic: {}", "Deleted.".red().bold(), topic);
    } else if let Some(id) = event_id {
        println!("{} Memory with event ID: {}", "Deleted.".red().bold(), id);
    }

    Ok(())
}

pub async fn cmd_search(
    backend: &Backend,
    nomen: Option<&Nomen>,
    query: &str,
    tier: Option<&str>,
    limit: usize,
    vector_weight: f32,
    text_weight: f32,
    aggregate: bool,
    graph_expand: bool,
    max_hops: usize,
) -> Result<()> {
    let mut params = json!({
        "query": query,
        "limit": limit,
        "retrieval": {
            "vector_weight": vector_weight,
            "text_weight": text_weight,
            "aggregate": aggregate,
            "graph_expand": graph_expand,
            "max_hops": max_hops,
        }
    });
    if let Some(tier) = tier {
        params["visibility"] = json!(tier);
    }

    let result = cli_dispatch(backend, nomen, "memory.search", &params).await?;
    let results = result["results"].as_array();

    if results.map(|r| r.is_empty()).unwrap_or(true) {
        println!("No results found for: {query}");
        return Ok(());
    }
    let results = results.unwrap();

    println!(
        "\n{} for \"{}\"\n{}",
        "Search Results".bold(),
        query,
        "═".repeat(60)
    );

    for (i, r) in results.iter().enumerate() {
        let vis = r["visibility"].as_str().unwrap_or("public");
        let tier_display = format!("[{}]", vis);
        let tier_colored = match vis {
            "public" => tier_display.green(),
            "personal" | "private" | "internal" => tier_display.red(),
            _ => tier_display.yellow(),
        };

        let match_type = r["match_type"].as_str().unwrap_or("");
        let graph_edge = r["graph_edge"].as_str();
        let match_indicator = match match_type {
            "graph" => match graph_edge {
                Some("contradicts") => " [graph:contradicts]",
                Some("mentions") => " [graph:mentions]",
                Some("references") => " [graph:references]",
                Some("consolidated_from") => " [graph:consolidated]",
                _ => " [graph]",
            },
            other => match other {
                "hybrid" => " [hybrid]",
                "vector" => " [vector]",
                "text" => " [text]",
                _ => "",
            },
        };

        let contradicts = r["contradicts"].as_bool().unwrap_or(false);
        let contradicts_prefix = if contradicts {
            format!("{} ", "[CONTRADICTS]".red().bold())
        } else {
            String::new()
        };

        let topic = r["topic"].as_str().unwrap_or("");
        let content = r["content"].as_str().unwrap_or("");
        let created_at = r["created_at"].as_str().unwrap_or("");

        println!(
            "\n{}. {} {}{}{}",
            i + 1,
            tier_colored,
            contradicts_prefix,
            topic.bold(),
            match_indicator.dimmed()
        );
        println!("   {}", content);
        println!("   Created: {}", created_at);
    }

    println!("\n{}: {} results\n", "Found".bold(), results.len());
    Ok(())
}
