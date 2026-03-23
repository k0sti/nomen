//! Group management CLI commands.

use anyhow::Result;
use colored::Colorize;
use serde_json::json;

use nomen::Nomen;

use super::helpers::{cli_dispatch, Backend};
use super::GroupAction;

pub async fn cmd_group(backend: &Backend, nomen: Option<&Nomen>, action: GroupAction) -> Result<()> {
    match action {
        GroupAction::Create {
            id,
            name,
            members,
            nostr_group,
            relay,
        } => {
            let params = json!({
                "id": id,
                "name": name,
                "members": members,
                "nostr_group": nostr_group,
                "relay": relay,
            });
            cli_dispatch(backend, nomen, "group.create", &params).await?;
            println!(
                "{} group: {} ({})",
                "Created".green().bold(),
                id.bold(),
                name
            );
            if !members.is_empty() {
                println!("  Members: {}", members.join(", "));
            }
        }
        GroupAction::List => {
            let result = cli_dispatch(backend, nomen, "group.list", &json!({})).await?;
            let groups = result["groups"].as_array();

            if groups.map(|g| g.is_empty()).unwrap_or(true) {
                println!("No groups configured.");
                return Ok(());
            }
            let groups = groups.unwrap();

            println!("\n{}\n{}", "Groups".bold(), "═".repeat(60));

            for group in groups {
                let id = group["id"].as_str().unwrap_or("");
                let name = group["name"].as_str().unwrap_or("");
                let parent = group["parent"].as_str().unwrap_or("");
                let nostr_group = group["nostr_group"].as_str().unwrap_or("");
                let member_count = group["members"].as_array().map(|m| m.len()).unwrap_or(0);

                let parent_display = if parent.is_empty() {
                    String::new()
                } else {
                    format!(" (parent: {})", parent)
                };
                let nostr_display = if nostr_group.is_empty() {
                    String::new()
                } else {
                    format!(" [NIP-29: {}]", nostr_group)
                };

                println!(
                    "\n  {} — {}{}{}",
                    id.bold(),
                    name,
                    parent_display,
                    nostr_display.dimmed()
                );
                println!(
                    "    Members: {}",
                    if member_count == 0 {
                        "(none)".to_string()
                    } else {
                        format!("{} member(s)", member_count)
                    }
                );
            }
            println!();
        }
        GroupAction::Members { id } => {
            let result = cli_dispatch(backend, nomen, "group.members", &json!({"id": id})).await?;
            let members = result["members"].as_array();
            println!("\n{} members of {}:\n", "Showing".bold(), id.bold());
            if let Some(members) = members {
                if members.is_empty() {
                    println!("  (no members)");
                } else {
                    for m in members {
                        println!("  {}", m.as_str().unwrap_or(""));
                    }
                }
            } else {
                println!("  (no members)");
            }
            println!();
        }
        GroupAction::AddMember { id, npub } => {
            cli_dispatch(backend, nomen, "group.add_member", &json!({"id": id, "npub": npub})).await?;
            println!("{} {} to group {}", "Added".green().bold(), npub, id.bold());
        }
        GroupAction::RemoveMember { id, npub } => {
            cli_dispatch(backend, nomen, "group.remove_member", &json!({"id": id, "npub": npub})).await?;
            println!(
                "{} {} from group {}",
                "Removed".red().bold(),
                npub,
                id.bold()
            );
        }
    }

    Ok(())
}
