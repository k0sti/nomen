use chrono::{TimeZone, Utc};
use colored::Colorize;
use nostr_sdk::Timestamp;

use crate::memory::ParsedMemory;

pub fn format_timestamp(ts: Timestamp) -> String {
    let secs = ts.as_u64() as i64;
    match Utc.timestamp_opt(secs, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        _ => format!("{secs}"),
    }
}

pub fn display_memories(npubs: &[String], memories: &[ParsedMemory], lesson_count: usize) {
    for npub in npubs {
        println!(
            "\n{}\n{}",
            format!("Memory Events for {npub}").bold(),
            "═".repeat(60)
        );
    }

    if memories.is_empty() {
        println!("\n  No memory events found.\n");
        return;
    }

    let mut public_count = 0usize;
    let mut group_count = 0usize;
    let mut personal_count = 0usize;

    for mem in memories {
        match mem.visibility.as_str() {
            "public" => public_count += 1,
            "personal" | "internal" | "private" => personal_count += 1,
            t if t.starts_with("group") => group_count += 1,
            _ => public_count += 1,
        }
    }

    for mem in memories {
        let tier_display = format!("[{}]", mem.visibility);
        let tier_colored = match mem.visibility.as_str() {
            "public" => tier_display.green(),
            "personal" | "internal" | "private" => tier_display.red(),
            _ => tier_display.yellow(),
        };

        println!(
            "\n{} {}",
            tier_colored,
            mem.topic.bold(),
        );
        println!("  Model: {}", mem.model);
        println!("  Summary: {}", crate::memory::first_line(&mem.content));
        println!("  Created: {}", format_timestamp(mem.created_at));
    }

    println!(
        "\n{}: {} memories ({} public, {} group, {} personal), {} lessons\n",
        "Total".bold(),
        memories.len(),
        public_count,
        group_count,
        personal_count,
        lesson_count
    );
}
