/**
 * OpenClaw Memory (Nomen) Plugin
 *
 * Replaces the built-in file-backed memory with Nomen's
 * hybrid search (vector + FTS + graph) over HTTP API.
 */

// ── Config ──────────────────────────────────────────────────────────

type NomenConfig = {
  apiUrl: string;
  visibility: string;
  timeoutMs: number;
};

const DEFAULT_CONFIG: NomenConfig = {
  apiUrl: "http://127.0.0.1:3849/memory/api/dispatch",
  visibility: "internal",
  timeoutMs: 10000,
};

// ── Nomen API client ────────────────────────────────────────────────

type NomenResponse = {
  ok: boolean;
  result?: Record<string, unknown>;
  error?: { code: string; message: string };
};

async function nomenRequest(
  apiUrl: string,
  action: string,
  params: Record<string, unknown>,
  timeoutMs: number,
): Promise<NomenResponse> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);

  try {
    const res = await fetch(apiUrl, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ action, params }),
      signal: controller.signal,
    });

    if (!res.ok) {
      return {
        ok: false,
        error: { code: "http_error", message: `HTTP ${res.status}` },
      };
    }

    return (await res.json()) as NomenResponse;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return {
      ok: false,
      error: { code: "request_failed", message },
    };
  } finally {
    clearTimeout(timer);
  }
}

// ── Result formatting ───────────────────────────────────────────────

type NomenSearchResult = {
  topic: string;
  summary: string;
  detail: string;
  visibility: string;
  confidence: string;
  match_type: string;
  created_at: string;
};

function formatSearchResults(results: NomenSearchResult[]) {
  return {
    results: results.map((r) => ({
      path: `nomen:${r.topic}`,
      snippet: r.detail || r.summary,
      score: parseFloat(r.confidence) || 0.5,
      topic: r.topic,
      visibility: r.visibility,
      matchType: r.match_type,
      createdAt: r.created_at,
    })),
  };
}

function jsonResult(data: Record<string, unknown>) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(data) }],
  };
}

// ── Plugin ───────────────────────────────────────────────────────────

const memoryNomenPlugin = {
  id: "memory-nomen",
  name: "Memory (Nomen)",
  description:
    "Nomen-backed memory with hybrid search (vector + FTS + knowledge graph)",
  kind: "memory" as const,
  configSchema: {
    type: "object" as const,
    additionalProperties: false,
    properties: {},
    parse: (v: unknown) => v,
  },

  register(api: any) {
    const rawConfig = api.pluginConfig ?? {};
    const cfg: NomenConfig = {
      apiUrl: rawConfig.apiUrl ?? DEFAULT_CONFIG.apiUrl,
      visibility: rawConfig.visibility ?? DEFAULT_CONFIG.visibility,
      timeoutMs: rawConfig.timeoutMs ?? DEFAULT_CONFIG.timeoutMs,
    };

    api.logger.info(
      `memory-nomen: registered (api: ${cfg.apiUrl}, visibility: ${cfg.visibility})`,
    );

    // ── memory_search ─────────────────────────────────────────────

    api.registerTool(
      {
        name: "memory_search",
        label: "Memory Search",
        description:
          "Mandatory recall step: semantically search Nomen memory before answering questions about prior work, decisions, dates, people, preferences, or todos; returns top snippets with path + lines. If response has disabled=true, memory retrieval is unavailable and should be surfaced to the user.",
        parameters: {
          type: "object",
          properties: {
            query: { type: "string", description: "Search query" },
            maxResults: { type: "number", description: "Max results (default: 10)" },
            minScore: { type: "number", description: "Minimum confidence score" },
          },
          required: ["query"],
        },
        async execute(_toolCallId: string, params: any) {
          const query = params?.query;
          const maxResults = params?.maxResults ?? 10;

          if (!query) {
            return jsonResult({ disabled: true, error: "query is required" });
          }

          const resp = await nomenRequest(
            cfg.apiUrl,
            "memory.search",
            { query, limit: maxResults },
            cfg.timeoutMs,
          );

          if (!resp.ok) {
            const msg = resp.error?.message ?? "Nomen memory unavailable";
            api.logger.warn(`memory-nomen: search failed: ${msg}`);
            return jsonResult({ disabled: true, error: msg });
          }

          const rawResults =
            ((resp.result as any)?.results as NomenSearchResult[]) ?? [];

          const formatted = formatSearchResults(rawResults);

          return jsonResult({
            ...formatted,
            provider: "nomen",
            model: "hybrid",
            citations: "auto",
          });
        },
      },
      { names: ["memory_search"] },
    );

    // ── memory_get ────────────────────────────────────────────────

    api.registerTool(
      {
        name: "memory_get",
        label: "Memory Get",
        description:
          "Safe snippet read from MEMORY.md or memory/*.md with optional from/lines; use after memory_search to pull only the needed lines and keep context small.",
        parameters: {
          type: "object",
          properties: {
            path: {
              type: "string",
              description:
                "Memory topic (e.g. 'projects/snowclaw') or nomen:topic path from search results",
            },
            from: { type: "number", description: "Unused (compat)" },
            lines: { type: "number", description: "Unused (compat)" },
          },
          required: ["path"],
        },
        async execute(_toolCallId: string, params: any) {
          const rawPath = params?.path ?? "";
          const topic = rawPath.replace(/^nomen:/, "");

          // Try by topic
          const resp = await nomenRequest(
            cfg.apiUrl,
            "memory.get",
            { topic },
            cfg.timeoutMs,
          );

          if (resp.ok && resp.result && (resp.result as any).topic != null) {
            const r = resp.result as Record<string, unknown>;
            return jsonResult({
              path: `nomen:${r.topic}`,
              text: (r.detail as string) || (r.summary as string) || "",
              topic: r.topic,
              visibility: r.visibility,
              confidence: r.confidence,
            });
          }

          // Fallback: try as d_tag
          const resp2 = await nomenRequest(
            cfg.apiUrl,
            "memory.get",
            { d_tag: topic },
            cfg.timeoutMs,
          );

          if (resp2.ok && resp2.result && (resp2.result as any).topic != null) {
            const r = resp2.result as Record<string, unknown>;
            return jsonResult({
              path: `nomen:${r.topic}`,
              text: (r.detail as string) || (r.summary as string) || "",
              topic: r.topic,
              visibility: r.visibility,
              confidence: r.confidence,
            });
          }

          // Not found — fall back to file read for backward compat
          // (MEMORY.md, memory/*.md still exist on disk)
          try {
            const fs = await import("node:fs");
            const path = await import("node:path");
            const workspace = api.runtime?.config?.agents?.defaults?.workspace ?? process.cwd();
            const filePath = path.resolve(workspace, rawPath);

            if (fs.existsSync(filePath)) {
              const content = fs.readFileSync(filePath, "utf8");
              const from = params?.from ? Math.max(1, params.from) : 1;
              const lines = params?.lines;
              const allLines = content.split("\n");
              const start = from - 1;
              const end = lines ? start + lines : allLines.length;
              const slice = allLines.slice(start, end).join("\n");

              return jsonResult({
                path: rawPath,
                text: slice,
                fromLine: from,
                totalLines: allLines.length,
              });
            }
          } catch {
            // ignore file read errors
          }

          return jsonResult({
            path: rawPath,
            text: "",
            error: "Memory not found",
          });
        },
      },
      { names: ["memory_get"] },
    );

    // ── memory_consolidate_prepare ───────────────────────────────

    api.registerTool(
      {
        name: "memory_consolidate_prepare",
        label: "Memory Consolidate Prepare",
        description:
          "Prepare consolidation batches for two-phase agent mode. Returns grouped message batches for external LLM processing.",
        parameters: {
          type: "object",
          properties: {
            batch_size: { type: "number", description: "Max messages per batch (default 50)" },
            min_messages: { type: "number", description: "Min messages to trigger (default 3)" },
            ttl_minutes: { type: "number", description: "Session TTL in minutes (default 60)" },
          },
        },
        async execute(_toolCallId: string, params: any) {
          const resp = await nomenRequest(
            cfg.apiUrl,
            "memory.consolidate_prepare",
            {
              batch_size: params?.batch_size ?? 50,
              min_messages: params?.min_messages ?? 3,
              ttl_minutes: params?.ttl_minutes ?? 60,
            },
            cfg.timeoutMs,
          );

          if (!resp.ok) {
            return jsonResult({ error: resp.error?.message ?? "prepare failed" });
          }

          return jsonResult(resp.result as Record<string, unknown>);
        },
      },
      { names: ["memory_consolidate_prepare"] },
    );

    // ── memory_consolidate_commit ─────────────────────────────────

    api.registerTool(
      {
        name: "memory_consolidate_commit",
        label: "Memory Consolidate Commit",
        description:
          "Commit agent-provided extractions for a prepared consolidation session. Runs storage, graph edges, and cleanup.",
        parameters: {
          type: "object",
          properties: {
            session_id: { type: "string", description: "Session ID from consolidate_prepare" },
            extractions: {
              type: "array",
              description: "Array of batch extractions",
              items: {
                type: "object",
                properties: {
                  batch_id: { type: "string" },
                  memories: {
                    type: "array",
                    items: {
                      type: "object",
                      properties: {
                        topic: { type: "string" },
                        summary: { type: "string" },
                        detail: { type: "string" },
                        importance: { type: "number" },
                      },
                      required: ["topic", "summary", "importance"],
                    },
                  },
                },
                required: ["batch_id", "memories"],
              },
            },
          },
          required: ["session_id", "extractions"],
        },
        async execute(_toolCallId: string, params: any) {
          if (!params?.session_id || !params?.extractions) {
            return jsonResult({ error: "session_id and extractions are required" });
          }

          const resp = await nomenRequest(
            cfg.apiUrl,
            "memory.consolidate_commit",
            {
              session_id: params.session_id,
              extractions: params.extractions,
            },
            cfg.timeoutMs,
          );

          if (!resp.ok) {
            return jsonResult({ error: resp.error?.message ?? "commit failed" });
          }

          return jsonResult(resp.result as Record<string, unknown>);
        },
      },
      { names: ["memory_consolidate_commit"] },
    );

    // ── Message ingestion hooks ─────────────────────────────────

    // Inbound messages
    api.registerHook(
      "message:received",
      async (event: any) => {
        const ctx = event?.context;
        if (!ctx?.content) return;

        try {
          await nomenRequest(
            cfg.apiUrl,
            "message.ingest",
            {
              source: ctx.messageId || `${ctx.channelId}:${Date.now()}`,
              sender: ctx.from || "unknown",
              content: ctx.content,
              channel: ctx.conversationId
                ? `${ctx.channelId}:${ctx.conversationId}`
                : ctx.channelId || "unknown",
            },
            cfg.timeoutMs,
          );
        } catch (err) {
          api.logger.warn(
            `memory-nomen: ingest failed: ${err instanceof Error ? err.message : err}`,
          );
        }
      },
      { name: "nomen-ingest-received" },
    );

    // Outbound messages
    api.registerHook(
      "message:sent",
      async (event: any) => {
        const ctx = event?.context;
        if (!ctx?.content) return;

        try {
          await nomenRequest(
            cfg.apiUrl,
            "message.ingest",
            {
              source: ctx.messageId || `${ctx.channelId}:sent:${Date.now()}`,
              sender: "clarity",
              content: ctx.content,
              channel: ctx.conversationId
                ? `${ctx.channelId}:${ctx.conversationId}`
                : ctx.channelId || "unknown",
            },
            cfg.timeoutMs,
          );
        } catch (err) {
          api.logger.warn(
            `memory-nomen: ingest failed: ${err instanceof Error ? err.message : err}`,
          );
        }
      },
      { name: "nomen-ingest-sent" },
    );

    // ── CLI ───────────────────────────────────────────────────────

    api.registerCli(
      ({ program }: any) => {
        const memory = program
          .command("memory")
          .description("Nomen memory commands");

        memory
          .command("status")
          .description("Show Nomen memory status")
          .action(async () => {
            const resp = await nomenRequest(
              cfg.apiUrl,
              "memory.list",
              {},
              cfg.timeoutMs,
            );
            if (!resp.ok) {
              console.error("Nomen unreachable:", resp.error?.message);
              process.exit(1);
            }
            const count = (resp.result as any)?.count ?? "?";
            console.log(`Nomen memory: ${count} memories`);
            console.log(`API: ${cfg.apiUrl}`);
            console.log(`Default visibility: ${cfg.visibility}`);
          });

        memory
          .command("search")
          .description("Search Nomen memories")
          .argument("<query>", "Search query")
          .option("--limit <n>", "Max results", "10")
          .action(async (query: string, opts: { limit: string }) => {
            const resp = await nomenRequest(
              cfg.apiUrl,
              "memory.search",
              { query, limit: parseInt(opts.limit) },
              cfg.timeoutMs,
            );
            if (!resp.ok) {
              console.error("Search failed:", resp.error?.message);
              process.exit(1);
            }
            const results =
              (((resp.result as any)?.results ?? []) as NomenSearchResult[]);
            if (results.length === 0) {
              console.log("No results.");
              return;
            }
            for (const r of results) {
              console.log(
                `[${r.visibility}] ${r.topic} (${r.match_type}, ${r.confidence})`,
              );
              console.log(`  ${r.summary}`);
              console.log();
            }
          });
      },
      { commands: ["memory"] },
    );
  },
};

export default memoryNomenPlugin;
