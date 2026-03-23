//! Message-related CLI commands: ingest, messages, delete-ephemeral, send.

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::json;
use tracing::debug;

use nomen::Nomen;

use super::helpers::{cli_dispatch, Backend};

pub async fn cmd_ingest(
    backend: &Backend,
    nomen: Option<&Nomen>,
    content: &str,
    source: &str,
    sender: &str,
    channel: Option<&str>,
) -> Result<()> {
    let params = json!({
        "content": content,
        "source": source,
        "sender": sender,
        "channel": channel,
    });

    let result = cli_dispatch(backend, nomen, "message.ingest", &params).await?;
    let id = result["id"].as_str().unwrap_or("");
    println!(
        "{} message from {} [{}]{}",
        "Ingested".green().bold(),
        sender.bold(),
        source,
        channel.map(|c| format!(" #{c}")).unwrap_or_default()
    );
    debug!("Record ID: {id}");
    Ok(())
}

pub async fn cmd_messages(
    backend: &Backend,
    nomen: Option<&Nomen>,
    source: Option<&str>,
    channel: Option<&str>,
    sender: Option<&str>,
    since: Option<&str>,
    limit: usize,
    around: Option<&str>,
    context_count: usize,
) -> Result<()> {
    let result = if let Some(source_id) = around {
        let params = json!({
            "source_id": source_id,
            "before": context_count,
            "after": context_count,
        });
        cli_dispatch(backend, nomen, "message.context", &params).await?
    } else {
        let params = json!({
            "source": source,
            "channel": channel,
            "sender": sender,
            "since": since,
            "limit": limit,
        });
        cli_dispatch(backend, nomen, "message.list", &params).await?
    };

    let messages = result["messages"].as_array();
    if messages.map(|m| m.is_empty()).unwrap_or(true) {
        println!("No messages found.");
        return Ok(());
    }
    let messages = messages.unwrap();

    println!("\n{}\n{}", "Raw Messages".bold(), "═".repeat(60));

    for msg in messages {
        let msg_source = msg["source"].as_str().unwrap_or("");
        let msg_sender = msg["sender"].as_str().unwrap_or("");
        let msg_channel = msg["channel"].as_str().unwrap_or("");
        let msg_content = msg["content"].as_str().unwrap_or("");
        let msg_created = msg["created_at"].as_str().unwrap_or("");
        let consolidated = msg["consolidated"].as_bool().unwrap_or(false);

        let channel_display = if msg_channel.is_empty() {
            String::new()
        } else {
            format!(" #{}", msg_channel)
        };
        let consolidated_marker = if consolidated {
            " [consolidated]".dimmed().to_string()
        } else {
            String::new()
        };

        println!(
            "\n  [{}] {}{}{}\n    {}",
            msg_source,
            msg_sender.bold(),
            channel_display,
            consolidated_marker,
            msg_content
        );
        println!("    {}", msg_created.dimmed());
    }

    println!("\n{}: {} messages\n", "Total".bold(), messages.len());
    Ok(())
}

pub async fn cmd_delete_ephemeral(backend: &Backend, nomen: Option<&Nomen>, older_than: Option<&str>) -> Result<()> {
    if matches!(backend, Backend::Http(_)) {
        bail!("This command requires direct DB access. Stop the nomen service first.");
    }
    let older_than = older_than.ok_or_else(|| {
        anyhow::anyhow!("--older-than is required with --ephemeral (e.g. --older-than 7d)")
    })?;

    let nomen = nomen.expect("Direct backend requires a Nomen instance");
    let count = nomen.delete_ephemeral(older_than).await?;

    if count == 0 {
        println!("No ephemeral messages older than {older_than} to delete.");
    } else {
        println!(
            "{}: {} ephemeral messages deleted (older than {older_than})",
            "Deleted".red().bold(),
            count
        );
    }

    Ok(())
}

pub async fn cmd_send(
    backend: &Backend,
    nomen: Option<&Nomen>,
    recipient: &str,
    content: &str,
    channel: Option<&str>,
) -> Result<()> {
    let params = json!({
        "recipient": recipient,
        "content": content,
        "channel": channel,
    });

    let result = cli_dispatch(backend, nomen, "message.send", &params).await?;
    let event_id = result["event_id"].as_str().unwrap_or("");
    let summary = result["summary"].as_str().unwrap_or("");

    println!(
        "{} to {}: event_id={}",
        "Sent".green().bold(),
        recipient.bold(),
        event_id
    );
    if !summary.is_empty() {
        println!("  {}", summary);
    }

    Ok(())
}
