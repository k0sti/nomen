use anyhow::Result;
use serde::Deserialize;
use surrealdb::engine::local::Db;
use surrealdb::types::{RecordId, SurrealValue};
use surrealdb::Surreal;

/// A memory discovered through graph edge traversal.
#[derive(Debug, Deserialize, SurrealValue)]
pub struct GraphNeighbor {
    /// Edge type: "mentions", "references", "consolidated_from", or "contradicts"
    pub edge_type: String,
    /// The relation field on references edges (e.g. "contradicts", "supersedes")
    pub relation: Option<String>,
    pub tier: String,
    pub topic: String,
    pub content: String,
    pub created_at: String,
    pub d_tag: Option<String>,
    pub importance: Option<i64>,
    pub last_accessed: Option<String>,
}

/// Create a "references" edge between two memories with a relation type.
pub async fn create_references_edge(
    db: &Surreal<Db>,
    from_d_tag: &str,
    to_d_tag: &str,
    relation: &str,
) -> Result<()> {
    // Resolve d_tags to record IDs
    #[derive(Deserialize, SurrealValue)]
    struct IdRow {
        id: RecordId,
    }
    let from_rows: Vec<IdRow> = db
        .query("SELECT id FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", from_d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;
    let to_rows: Vec<IdRow> = db
        .query("SELECT id FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", to_d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;

    let from_id = from_rows
        .first()
        .map(|r| &r.id)
        .ok_or_else(|| anyhow::anyhow!("Memory not found: {from_d_tag}"))?;
    let to_id = to_rows
        .first()
        .map(|r| &r.id)
        .ok_or_else(|| anyhow::anyhow!("Memory not found: {to_d_tag}"))?;

    db.query("RELATE $from->references->$to SET relation = $relation, created_at = $now")
        .bind(("from", from_id.clone()))
        .bind(("to", to_id.clone()))
        .bind(("relation", relation.to_string()))
        .bind(("now", chrono::Utc::now().to_rfc3339()))
        .await?
        .check()?;
    Ok(())
}

/// Create a "consolidated_from" edge from a consolidated memory to a collected message.
pub async fn create_consolidated_edge(
    db: &Surreal<Db>,
    memory_id: &str,
    message_d_tag: &str,
) -> Result<()> {
    // Use d_tag-based lookup since collected_message uses SurrealDB auto-generated IDs
    db.query(
        "LET $msg = (SELECT id FROM collected_message WHERE d_tag = $dtag LIMIT 1); \
         IF $msg[0] != NONE THEN \
           (RELATE $from->consolidated_from->$msg[0].id) \
         END"
    )
        .bind(("from", RecordId::new("memory", memory_id)))
        .bind(("dtag", message_d_tag.to_string()))
        .await?
        .check()?;
    Ok(())
}

/// Traverse 1-hop outgoing and incoming graph edges
/// from a memory identified by d_tag. This is a simpler, more reliable query than the full
/// graph traversal.
pub async fn get_graph_neighbors_simple(
    db: &Surreal<Db>,
    d_tag: &str,
) -> Result<Vec<GraphNeighbor>> {
    let mut all: Vec<GraphNeighbor> = Vec::new();

    // Find the memory record ID first
    #[derive(Deserialize, SurrealValue)]
    struct IdRow {
        id: RecordId,
    }
    let rows: Vec<IdRow> = db
        .query("SELECT id FROM memory WHERE d_tag = $d_tag LIMIT 1")
        .bind(("d_tag", d_tag.to_string()))
        .await?
        .check()?
        .take(0)?;

    let thing = match rows.first() {
        Some(r) => r.id.clone(),
        None => return Ok(all),
    };

    // 1. Outgoing references: memory->references->memory
    #[derive(Debug, Deserialize, SurrealValue)]
    struct RefEdge {
        relation: Option<String>,
        out: RecordId,
    }
    let out_edges: Vec<RefEdge> = db
        .query("SELECT relation, out FROM references WHERE in = $mid")
        .bind(("mid", thing.clone()))
        .await?
        .check()?
        .take(0)?;

    for edge in &out_edges {
        let mems: Vec<GraphNeighbor> = db
            .query("SELECT $edge_type AS edge_type, $relation AS relation, tier, topic, content, created_at, d_tag, importance, last_accessed FROM $target")
            .bind(("target", edge.out.clone()))
            .bind(("edge_type", "references".to_string()))
            .bind(("relation", edge.relation.clone().unwrap_or_default()))
            .await?
            .check()?
            .take(0)?;
        all.extend(mems);
    }

    // 2. Incoming references: memory<-references<-memory
    #[derive(Debug, Deserialize, SurrealValue)]
    struct RefEdgeIn {
        relation: Option<String>,
        #[serde(rename = "in")]
        #[surreal(rename = "in")]
        in_node: RecordId,
    }
    let in_edges: Vec<RefEdgeIn> = db
        .query("SELECT relation, in FROM references WHERE out = $mid")
        .bind(("mid", thing.clone()))
        .await?
        .check()?
        .take(0)?;

    for edge in &in_edges {
        let mems: Vec<GraphNeighbor> = db
            .query("SELECT $edge_type AS edge_type, $relation AS relation, tier, topic, content, created_at, d_tag, importance, last_accessed FROM $target")
            .bind(("target", edge.in_node.clone()))
            .bind(("edge_type", "references".to_string()))
            .bind(("relation", edge.relation.clone().unwrap_or_default()))
            .await?
            .check()?
            .take(0)?;
        all.extend(mems);
    }

    // 3. Shared entity mentions: find entities this memory mentions, then find other memories mentioning those entities
    #[derive(Debug, Deserialize, SurrealValue)]
    struct MentionEdge {
        out: RecordId,
    }
    let mention_edges: Vec<MentionEdge> = db
        .query("SELECT out FROM mentions WHERE in = $mid")
        .bind(("mid", thing.clone()))
        .await?
        .check()?
        .take(0)?;

    for mention in &mention_edges {
        // Find other memories that also mention this entity
        #[derive(Debug, Deserialize, SurrealValue)]
        struct MentionBack {
            #[serde(rename = "in")]
            #[surreal(rename = "in")]
            in_node: RecordId,
        }
        let back_edges: Vec<MentionBack> = db
            .query("SELECT in FROM mentions WHERE out = $ent AND in != $mid")
            .bind(("ent", mention.out.clone()))
            .bind(("mid", thing.clone()))
            .await?
            .check()?
            .take(0)?;

        for back in &back_edges {
            let mems: Vec<GraphNeighbor> = db
                .query("SELECT $edge_type AS edge_type, NONE AS relation, tier, topic, content, created_at, d_tag, importance, last_accessed FROM $target")
                .bind(("target", back.in_node.clone()))
                .bind(("edge_type", "mentions".to_string()))
                .await?
                .check()?
                .take(0)?;
            all.extend(mems);
        }
    }

    // 4. Consolidated_from siblings: memories that share the same raw message sources
    #[derive(Debug, Deserialize, SurrealValue)]
    struct ConsolidatedEdge {
        out: RecordId,
    }
    let consolidated_edges: Vec<ConsolidatedEdge> = db
        .query("SELECT out FROM consolidated_from WHERE in = $mid")
        .bind(("mid", thing.clone()))
        .await?
        .check()?
        .take(0)?;

    for consol in &consolidated_edges {
        #[derive(Debug, Deserialize, SurrealValue)]
        struct ConsolBack {
            #[serde(rename = "in")]
            #[surreal(rename = "in")]
            in_node: RecordId,
        }
        let back_edges: Vec<ConsolBack> = db
            .query("SELECT in FROM consolidated_from WHERE out = $raw AND in != $mid")
            .bind(("raw", consol.out.clone()))
            .bind(("mid", thing.clone()))
            .await?
            .check()?
            .take(0)?;

        for back in &back_edges {
            let mems: Vec<GraphNeighbor> = db
                .query("SELECT $edge_type AS edge_type, NONE AS relation, tier, topic, content, created_at, d_tag, importance, last_accessed FROM $target")
                .bind(("target", back.in_node.clone()))
                .bind(("edge_type", "consolidated_from".to_string()))
                .await?
                .check()?
                .take(0)?;
            all.extend(mems);
        }
    }

    Ok(all)
}
