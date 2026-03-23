mod grouping;
mod llm;
mod pipeline;
mod two_phase;
mod types;

// Re-export all public items to maintain the existing API surface.
// Everything that was previously `crate::consolidate::Foo` stays accessible.

// Types
pub use types::{
    AgentExtractedMemory, BatchExtraction, BatchMessage, CommitResult, ConsolidationConfig,
    ConsolidationReport, ConsolidationStatus, ExtractedMemory, GroupSummary, PrepareResult,
    PreparedBatch, TimeRange,
};

// Utility
pub use types::parse_duration_str;

// LLM provider trait and implementations
pub use llm::{LlmProvider, NoopLlmProvider, OpenAiLlmProvider};

// Main pipeline
pub use pipeline::{check_consolidation_due, consolidate, record_consolidation_run};

// Two-phase consolidation
pub use two_phase::{commit, prepare};
