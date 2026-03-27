//! Message-related CLI commands: ingest, messages, delete-ephemeral, send.

use anyhow::{bail, Result};
use colored::Colorize;
use serde_json::json;
use tracing::debug;

use nomen::Nomen;

use super::helpers::{cli_dispatch, Backend};

/// Ingest a collected message using canonical platform/community/chat/thread
/// fields.
pub async fn cmd_ingest(
    backend: &Backend,
    nomen: Option<&Nomen>,
    content: &str,
    source: &str,
    sender: &str,
    platform: Option<&str>,
    community: Option<&str>,
    chat: Option<&str>,
    thread: Option<&str>,
) -> Result<()> {
    let params = json!({
        "content": content,
        "source": source,
        "sender": sender,
        "platform": platform,
        "community_id": community,
        "chat_id": chat,
        "thread_id": thread,
    });

    let result = cli_dispatch(backend, nomen, "message.ingest", &params).await?;
    let d_tag = result["d_tag"].as_str().unwrap_or("");
    let location = if let Some(thread) = thread {
        format!(" #{} / {}", chat.unwrap_or("chat"), thread)
    } else {
        chat.map(|c| format!(" #{c}")).unwrap_or_default()
    };
    println!(
        "{} message from {} [{}]{}",
        "Ingested".green().bold(),
        sender.bold(),
        platform.unwrap_or(source),
        location
    );
    debug!("d_tag: {d_tag}");
    Ok(())
}

pub async fn cmd_messages(
    backend: &Backend,
    nomen: Option<&Nomen>,
    platform: Option<&str>,
    chat: Option<&str>,
    thread: Option<&str>,
    sender: Option<&str>,
    since: Option<&str>,
    limit: usize,
    around: Option<&str>,
    context_count: usize,
) -> Result<()> {
    let result = if let Some(_around_id) = around {
        // Context query requires the primary chat/container identity.
        if chat.is_none() {
            bail!("--around requires --chat to be set");
        }
        let params = json!({
            "#chat": [chat.unwrap()],
            "limit": context_count * 2 + 1,
        });
        cli_dispatch(backend, nomen, "message.context", &params).await?
    } else {
        let mut params = json!({ "limit": limit });
        if let Some(p) = platform {
            params["#proxy"] = json!([p]);
        }
        if let Some(c) = chat {
            params["#chat"] = json!([c]);
        }
        if let Some(t) = thread {
            params["#thread"] = json!([t]);
        }
        if let Some(s) = sender {
            params["#sender"] = json!([s]);
        }
        if let Some(s) = since {
            if let Ok(ts) = s.parse::<i64>() {
                params["since"] = json!(ts);
            }
        }
        cli_dispatch(backend, nomen, "message.query", &params).await?
    };

    // message.query returns { events: [...] }, message.context returns { messages: [...] }
    let events = result["events"]
        .as_array()
        .or_else(|| result["messages"].as_array());
    if events.map(|m| m.is_empty()).unwrap_or(true) {
        println!("No messages found.");
        return Ok(());
    }
    let events = events.unwrap();

    println!("\n{}\n{}", "Messages".bold(), "═".repeat(60));

    for event in events {
        // Try 30100 event format first (from message.query)
        let tags = event["tags"].as_array();
        if let Some(tags) = tags {
            let sender_tag = tags.iter().find(|t| t[0] == "sender");
            let chat_tag = tags.iter().find(|t| t[0] == "chat");
            let proxy_tag = tags.iter().find(|t| t[0] == "proxy");

            let msg_sender = sender_tag.and_then(|t| t[1].as_str()).unwrap_or("");
            let msg_chat = chat_tag.and_then(|t| t[1].as_str()).unwrap_or("");
            let msg_platform = proxy_tag.and_then(|t| t[2].as_str()).unwrap_or("");
            let msg_content = event["content"].as_str().unwrap_or("");
            let msg_created = event["created_at"].as_i64().unwrap_or(0);

            let chat_display = if msg_chat.is_empty() {
                String::new()
            } else {
                format!(" #{msg_chat}")
            };

            println!(
                "\n  [{}] {}{}\n    {}",
                msg_platform,
                msg_sender.bold(),
                chat_display,
                msg_content
            );
            if msg_created > 0 {
                println!("    {}", format!("{msg_created}").dimmed());
            }
        } else {
            // Flat format (from message.context)
            let msg_sender = event["sender"].as_str().unwrap_or("");
            let msg_chat = event["chat"].as_str().unwrap_or("");
            let msg_platform = event["platform"].as_str().unwrap_or("");
            let msg_content = event["content"].as_str().unwrap_or("");
            let msg_created = event["created_at"].as_i64().unwrap_or(0);

            let chat_display = if msg_chat.is_empty() {
                String::new()
            } else {
                format!(" #{msg_chat}")
            };

            println!(
                "\n  [{}] {}{}\n    {}",
                msg_platform,
                msg_sender.bold(),
                chat_display,
                msg_content
            );
            if msg_created > 0 {
                println!("    {}", format!("{msg_created}").dimmed());
            }
        }
    }

    println!("\n{}: {} messages\n", "Total".bold(), events.len());
    Ok(())
}

pub async fn cmd_delete_old_messages(
    backend: &Backend,
    nomen: Option<&Nomen>,
    older_than: Option<&str>,
) -> Result<()> {
    if matches!(backend, Backend::Http(..)) {
        bail!("This command requires direct DB access. Stop the nomen service first.");
    }
    let older_than = older_than
        .ok_or_else(|| anyhow::anyhow!("--older-than is required (e.g. --older-than 7d)"))?;

    let nomen = nomen.expect("Direct backend requires a Nomen instance");
    let count = nomen.delete_old_messages(older_than).await?;

    if count == 0 {
        println!("No consolidated messages older than {older_than} to delete.");
    } else {
        println!(
            "{}: {} consolidated messages deleted (older than {older_than})",
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
