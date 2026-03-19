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

type NomenMemoryRecord = {
  topic?: string;
  summary?: string;
  detail?: string;
  visibility?: string;
  scope?: string;
  confidence?: string | number;
  version?: number;
  created_at?: string;
  d_tag?: string;
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

async function getMemoryByDTag(
  apiUrl: string,
  dTag: string,
  timeoutMs: number,
): Promise<NomenMemoryRecord | null> {
  const resp = await nomenRequest(apiUrl, "memory.get", { d_tag: dTag }, timeoutMs);
  if (!resp.ok || !resp.result) return null;
  const result = resp.result as any;
  if (!result || result.topic == null) return null;
  return result as NomenMemoryRecord;
}

async function resolveRoomsByProvider(
  apiUrl: string,
  providerId: string,
  timeoutMs: number,
): Promise<NomenMemoryRecord[]> {
  const resp = await nomenRequest(apiUrl, "room.resolve", { provider_id: providerId }, timeoutMs);
  if (!resp.ok || !resp.result) return [];
  const result = resp.result as any;
  return ((result?.results ?? []) as NomenMemoryRecord[]);
}

function formatRoomSection(title: string, record: NomenMemoryRecord): string {
  const body = record.detail || record.summary || "";
  return `# ${title}${record.summary ? ` (${record.summary})` : ""}\n\n${body}`;
}

// ── Session key → provider ID extraction ─────────────────────────────

/**
 * Extract a provider chat identifier from an OpenClaw session key.
 *
 * Session keys look like:
 *   agent:main:telegram:-1003821690204:group
 *   agent:main:nostr-nip29:techteam:group
 *   agent:main:telegram:60996061:direct
 *
 * We extract the channel:chatId portion (e.g. "telegram:-1003821690204").
 * Returns null for session keys we can't parse.
 */
function extractProviderIdFromSessionKey(sessionKey: string): string | null {
  if (!sessionKey) return null;

  const lower = sessionKey.toLowerCase();
  const parts = lower.split(":").filter(Boolean);

  // Expected examples:
  //   agent:main:telegram:group:-1003821690204:topic:8485
  //   agent:main:telegram:60996061:direct
  if (parts.length < 5 || parts[0] !== "agent") return null;

  const channel = parts[2];

  const topicIdx = parts.indexOf("topic");
  if (topicIdx > 4) {
    const chatIdParts = parts.slice(3, topicIdx);
    if (chatIdParts.length === 0) return null;
    return `${channel}:${chatIdParts.join(":")}`;
  }

  const chatType = parts[parts.length - 1];
  if (!["group", "direct", "dm", "channel"].includes(chatType)) return null;

  const chatIdParts = parts.slice(3, parts.length - 1);
  if (chatIdParts.length === 0) return null;

  return `${channel}:${chatIdParts.join(":")}`;
}

function extractTopicIdFromSessionKey(sessionKey: string): string | null {
  if (!sessionKey) return null;
  const parts = sessionKey.split(":").filter(Boolean);
  const topicIdx = parts.indexOf("topic");
  if (topicIdx >= 0 && topicIdx + 1 < parts.length) {
    const topicId = parts[topicIdx + 1];
    if (topicId && /^\d+$/.test(topicId)) return topicId;
  }
  return null;
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
    additionalProperties: true,
    properties: {
      apiUrl: { type: "string" as const },
      visibility: { type: "string" as const },
      timeoutMs: { type: "number" as const },
    },
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

    // ── Startup health check (non-blocking) ───────────────────────
    (async () => {
      const healthUrl = cfg.apiUrl.replace(/\/dispatch$/, "/health");
      try {
        const controller = new AbortController();
        const timer = setTimeout(() => controller.abort(), 5000);
        const res = await fetch(healthUrl, { signal: controller.signal });
        clearTimeout(timer);
        if (res.ok) {
          const data = await res.json() as any;
          api.logger.info(
            `memory-nomen: ✅ Nomen healthy (v${data.version ?? "?"}, ${data.memory_count ?? "?"} memories)`,
          );
        } else {
          api.logger.error(
            `memory-nomen: ⚠️ Nomen health check failed (HTTP ${res.status}) — memory will be unavailable`,
          );
        }
      } catch (err) {
        api.logger.error(
          `memory-nomen: ❌ Nomen unreachable at ${healthUrl} — memory will be unavailable. Is nomen.service running?`,
        );
      }
    })();

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

          return jsonResult({
            path: rawPath,
            text: "",
            error: "Memory not found",
          });
        },
      },
      { names: ["memory_get"] },
    );

    // ── memory_put ─────────────────────────────────────────────

    api.registerTool(
      {
        name: "memory_put",
        label: "Memory Put",
        description:
          "Store a memory in Nomen. Use for saving facts, decisions, lessons, or any knowledge worth remembering across sessions.",
        parameters: {
          type: "object",
          properties: {
            topic: {
              type: "string",
              description:
                "Memory topic path (e.g. 'projects/myapp', 'lessons/debugging', 'k0/preferences')",
            },
            summary: {
              type: "string",
              description: "Brief one-line summary of the memory",
            },
            detail: {
              type: "string",
              description: "Full detail content (markdown supported)",
            },
            visibility: {
              type: "string",
              enum: ["personal", "group", "public"],
              description: "Visibility tier (default: from plugin config)",
            },
            pinned: {
              type: "boolean",
              description: "Pin this memory so it is always injected into the system prompt (optional)",
            },
          },
          required: ["topic", "summary"],
        },
        async execute(_toolCallId: string, params: any) {
          const topic = params?.topic;
          const summary = params?.summary;

          if (!topic || !summary) {
            return jsonResult({ error: "topic and summary are required" });
          }

          const resp = await nomenRequest(
            cfg.apiUrl,
            "memory.put",
            {
              topic,
              summary,
              detail: params?.detail || "",
              tier: params?.visibility || cfg.visibility,
              ...(params?.pinned != null ? { pinned: params.pinned } : {}),
            },
            cfg.timeoutMs,
          );

          if (!resp.ok) {
            return jsonResult({
              error: resp.error?.message ?? "memory.put failed",
            });
          }

          return jsonResult({ stored: true, topic, ...(resp.result ?? {}) });
        },
      },
      { names: ["memory_put"] },
    );

    // ── memory_delete ─────────────────────────────────────────

    api.registerTool(
      {
        name: "memory_delete",
        label: "Memory Delete",
        description:
          "Delete a memory from Nomen by topic name or d_tag.",
        parameters: {
          type: "object",
          properties: {
            topic: {
              type: "string",
              description:
                "Memory topic (e.g. 'projects/myapp') or full d_tag to delete",
            },
          },
          required: ["topic"],
        },
        async execute(_toolCallId: string, params: any) {
          const topic = params?.topic;

          if (!topic) {
            return jsonResult({ error: "topic is required" });
          }

          // Try as topic first, then as d_tag
          const resp = await nomenRequest(
            cfg.apiUrl,
            "memory.delete",
            { topic },
            cfg.timeoutMs,
          );

          if (resp.ok) {
            return jsonResult({ deleted: true, topic });
          }

          // Fallback: try as d_tag
          const resp2 = await nomenRequest(
            cfg.apiUrl,
            "memory.delete",
            { d_tag: topic },
            cfg.timeoutMs,
          );

          if (resp2.ok) {
            return jsonResult({ deleted: true, d_tag: topic });
          }

          return jsonResult({
            error: resp2.error?.message ?? "memory.delete failed",
          });
        },
      },
      { names: ["memory_delete"] },
    );

    // ── memory_pin ────────────────────────────────────────────────

    api.registerTool(
      {
        name: "memory_pin",
        label: "Memory Pin",
        description:
          "Pin a memory so it is always injected into the system prompt at the start of every session.",
        parameters: {
          type: "object",
          properties: {
            topic: {
              type: "string",
              description: "Memory topic to pin (e.g. 'k0/profile')",
            },
          },
          required: ["topic"],
        },
        async execute(_toolCallId: string, params: any) {
          const topic = params?.topic;
          if (!topic) return jsonResult({ error: "topic is required" });

          const resp = await nomenRequest(
            cfg.apiUrl,
            "memory.pin",
            { topic },
            cfg.timeoutMs,
          );

          if (!resp.ok) {
            return jsonResult({ error: resp.error?.message ?? "pin failed" });
          }

          return jsonResult({ pinned: true, topic });
        },
      },
      { names: ["memory_pin"] },
    );

    // ── memory_unpin ──────────────────────────────────────────────

    api.registerTool(
      {
        name: "memory_unpin",
        label: "Memory Unpin",
        description:
          "Unpin a memory so it is no longer injected into the system prompt.",
        parameters: {
          type: "object",
          properties: {
            topic: {
              type: "string",
              description: "Memory topic to unpin (e.g. 'k0/profile')",
            },
          },
          required: ["topic"],
        },
        async execute(_toolCallId: string, params: any) {
          const topic = params?.topic;
          if (!topic) return jsonResult({ error: "topic is required" });

          const resp = await nomenRequest(
            cfg.apiUrl,
            "memory.unpin",
            { topic },
            cfg.timeoutMs,
          );

          if (!resp.ok) {
            return jsonResult({ error: resp.error?.message ?? "unpin failed" });
          }

          return jsonResult({ pinned: false, topic });
        },
      },
      { names: ["memory_unpin"] },
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

    // ── Context injection (before_prompt_build hook) ────────────
    // Injects: 1) Pinned memories (always), 2) Room context (when chatId available)

    api.on(
      "before_prompt_build",
      async (_event: any, ctx: any) => {
        const sections: string[] = [];

        // ── 1. Pinned memories (always injected) ────────────────
        try {
          const pinnedResp = await nomenRequest(
            cfg.apiUrl,
            "memory.list",
            { pinned: true, limit: 50 },
            cfg.timeoutMs,
          );

          if (pinnedResp.ok && pinnedResp.result) {
            const memories = ((pinnedResp.result as any)?.memories ?? []) as NomenMemoryRecord[];
            if (memories.length > 0) {
              const pinnedLines = memories.map((m) => {
                const summary = m.summary ?? "";
                const detail = m.detail ?? "";
                // Include detail only if it's short enough to be useful
                const body = detail && detail.length < 500 && detail !== summary
                  ? `${summary}\n${detail}`
                  : summary;
                return `## ${m.topic}\n${body}`;
              });
              sections.push(`# Pinned Memories\n\n${pinnedLines.join("\n\n")}`);
            }
          }
        } catch (err) {
          api.logger.warn(
            `memory-nomen: pinned memory injection failed: ${err instanceof Error ? err.message : err}`,
          );
        }

        // ── 2. Room context (when chatId available) ─────────────
        const sessionKey: string = ctx?.sessionKey ?? "";
        const inbound: any = ctx?.inboundContext ?? {};
        const chatId = inbound?.chatId ?? inbound?.chat_id ?? extractProviderIdFromSessionKey(sessionKey) ?? "";
        const threadId = inbound?.threadId ?? inbound?.thread_id ?? extractTopicIdFromSessionKey(sessionKey) ?? "";

        if (chatId) {
          try {
            // Group/chat layer
            const groupDTag = `group:${chatId}:room`;
            const groupRecord = await getMemoryByDTag(cfg.apiUrl, groupDTag, cfg.timeoutMs);
            let groupInjected = false;

            if (groupRecord) {
              sections.push(formatRoomSection("Room Context", groupRecord));
              groupInjected = true;
            }

            // Topic/thread layer
            let topicInjected = false;
            if (threadId) {
              const topicDTag = `group:${chatId}:room/${threadId}`;
              const topicRecord = await getMemoryByDTag(cfg.apiUrl, topicDTag, cfg.timeoutMs);
              if (topicRecord) {
                sections.push(formatRoomSection("Topic Context", topicRecord));
                topicInjected = true;
              }
            }

            if (groupInjected || topicInjected) {
              api.logger.info(
                `memory-nomen: injected room context for provider "${chatId}" (group=${groupInjected}, topic=${topicInjected})`,
              );
            }
          } catch (err) {
            api.logger.warn(
              `memory-nomen: room context injection failed: ${err instanceof Error ? err.message : err}`,
            );
          }
        }

        if (sections.length === 0) return {};

        return {
          appendSystemContext: sections.join("\n\n"),
        };
      },
    );

    // ── Message ingestion hooks ─────────────────────────────────

    // Inbound messages — use api.on() for typed hooks (not api.registerHook which is the old HOOK.md system)
    // Signature: (event: PluginHookMessageReceivedEvent, ctx: PluginHookMessageContext)
    // event = { from, content, timestamp?, metadata? }
    // ctx = { channelId, accountId?, conversationId? }
    api.on(
      "message_received",
      async (event: any, ctx: any) => {
        if (!event?.content) return;

        try {
          await nomenRequest(
            cfg.apiUrl,
            "message.ingest",
            {
              source: `${ctx?.channelId || "unknown"}:${event.timestamp || Date.now()}`,
              sender: event.from || "unknown",
              content: event.content,
              channel: ctx?.conversationId
                ? `${ctx.channelId}:${ctx.conversationId}`
                : ctx?.channelId || "unknown",
            },
            cfg.timeoutMs,
          );
        } catch (err) {
          api.logger.warn(
            `memory-nomen: ingest failed: ${err instanceof Error ? err.message : err}`,
          );
        }
      },
    );

    // Outbound messages
    // Signature: (event: PluginHookMessageSentEvent, ctx: PluginHookMessageContext)
    // event = { to, content, success, error? }
    // ctx = { channelId, accountId?, conversationId? }
    api.on(
      "message_sent",
      async (event: any, ctx: any) => {
        if (!event?.content || !event.success) return;

        try {
          await nomenRequest(
            cfg.apiUrl,
            "message.ingest",
            {
              source: `${ctx?.channelId || "unknown"}:sent:${Date.now()}`,
              sender: "clarity",
              content: event.content,
              channel: ctx?.conversationId
                ? `${ctx.channelId}:${ctx.conversationId}`
                : ctx?.channelId || "unknown",
            },
            cfg.timeoutMs,
          );
        } catch (err) {
          api.logger.warn(
            `memory-nomen: ingest failed: ${err instanceof Error ? err.message : err}`,
          );
        }
      },
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
