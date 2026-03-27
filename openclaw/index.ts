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
  nsec?: string;
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
  nsec?: string,
): Promise<NomenResponse> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);

  try {
    const res = await fetch(apiUrl, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(nsec ? { "Authorization": `Nostr ${nsec}` } : {}),
      },
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
  content: string;
  visibility: string;
  match_type: string;
  created_at: string;
};

type CollectedMessageEvent = {
  kind: number;
  created_at: number;
  pubkey: string;
  tags: string[][];
  content: string;
  id?: string;
  sig?: string;
};

function formatSearchResults(results: NomenSearchResult[]) {
  return {
    results: results.map((r) => ({
      path: `nomen:${r.topic}`,
      snippet: r.content,
      score: 0.5,
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

function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function normalizeThreadId(value: unknown): string | undefined {
  if (typeof value === "string" && value.trim()) return value.trim();
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  return undefined;
}

function resolveContainer(params: {
  channelId?: string;
  conversationId?: string;
  threadId?: unknown;
}): { platform: string; chatId: string; threadId?: string } {
  const platform = asString(params.channelId) ?? "unknown";
  const conversationId = asString(params.conversationId) ?? platform;
  const explicitThreadId = normalizeThreadId(params.threadId);

  if (platform === "telegram") {
    const stripTelegramPrefix = (value: string): string => {
      const trimmed = value.trim();
      return trimmed.startsWith("telegram:") ? trimmed.slice("telegram:".length) : trimmed;
    };

    const normalizedConversationId = stripTelegramPrefix(conversationId);
    const topicMatch = /^(.+):topic:(.+)$/.exec(normalizedConversationId);
    if (topicMatch) {
      return {
        platform,
        chatId: topicMatch[1] ?? normalizedConversationId,
        threadId: explicitThreadId ?? topicMatch[2],
      };
    }

    return {
      platform,
      chatId: normalizedConversationId,
      ...(explicitThreadId ? { threadId: explicitThreadId } : {}),
    };
  }

  return {
    platform,
    chatId: conversationId,
    ...(explicitThreadId ? { threadId: explicitThreadId } : {}),
  };
}

function buildCollectedMessageEvent(params: {
  platform: string;
  chatId: string;
  threadId?: string;
  messageId: string;
  content: string;
  senderId: string;
  senderName?: string;
  senderUsername?: string;
  collectorPubkey?: string;
  direction?: "inbound" | "outbound";
}): CollectedMessageEvent {
  const dTag = `${params.platform}:${params.chatId}:${params.messageId}`;
  const tags: string[][] = [
    ["d", dTag],
    ["proxy", dTag, params.platform],
    ["chat", params.chatId],
    ["sender", params.senderId],
  ];

  if (params.senderName || params.senderUsername) {
    tags[3] = [
      "sender",
      params.senderId,
      params.senderName ?? "",
      params.senderUsername ?? "",
    ];
  }
  if (params.threadId) tags.push(["thread", params.threadId]);
  if (params.direction) tags.push(["direction", params.direction]);

  return {
    kind: 30100,
    created_at: Math.floor(Date.now() / 1000),
    pubkey: params.collectorPubkey ?? "0000000000000000000000000000000000000000000000000000000000000000",
    tags,
    content: params.content,
  };
}

async function storeCollectedMessage(
  cfg: NomenConfig,
  event: CollectedMessageEvent,
): Promise<NomenResponse> {
  return await nomenRequest(cfg.apiUrl, "message.store", { event }, cfg.timeoutMs, cfg.nsec);
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
      nsec: rawConfig.nsec ?? undefined,
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
            cfg.nsec,
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
            cfg.nsec,
          );

          if (resp.ok && resp.result && (resp.result as any).topic != null) {
            const r = resp.result as Record<string, unknown>;
            return jsonResult({
              path: `nomen:${r.topic}`,
              text: (r.content as string) || "",
              topic: r.topic,
              visibility: r.visibility,
            });
          }

          // Fallback: try as d_tag
          const resp2 = await nomenRequest(
            cfg.apiUrl,
            "memory.get",
            { d_tag: topic },
            cfg.timeoutMs,
            cfg.nsec,
          );

          if (resp2.ok && resp2.result && (resp2.result as any).topic != null) {
            const r = resp2.result as Record<string, unknown>;
            return jsonResult({
              path: `nomen:${r.topic}`,
              text: (r.content as string) || "",
              topic: r.topic,
              visibility: r.visibility,
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
            cfg.nsec,
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
                        content: { type: "string" },
                        importance: { type: "number" },
                      },
                      required: ["topic", "content", "importance"],
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
            cfg.nsec,
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

    // Inbound messages — use api.on() for typed hooks (not api.registerHook which is the old HOOK.md system)
    // Signature: (event: PluginHookMessageReceivedEvent, ctx: PluginHookMessageContext)
    // event = { from, content, timestamp?, metadata? }
    // ctx = { channelId, accountId?, conversationId? }
    api.on(
      "message_received",
      async (event: any, ctx: any) => {
        if (!event?.content) return;

        api.logger.info(
          `memory-nomen: message_received hook channel=${ctx?.channelId ?? "-"} conversation=${ctx?.conversationId ?? "-"} thread=${String(event?.metadata?.threadId ?? "-")} messageId=${String(event?.metadata?.messageId ?? "-")}`,
        );

        const container = resolveContainer({
          channelId: ctx?.channelId,
          conversationId: ctx?.conversationId,
          threadId: event?.metadata?.threadId,
        });
        const messageId = asString(event?.metadata?.messageId) ?? String(event.timestamp || Date.now());
        const senderId = asString(event?.metadata?.senderId) ?? asString(event?.from) ?? "unknown";
        const senderName = asString(event?.metadata?.senderName);
        const senderUsername = asString(event?.metadata?.senderUsername);

        try {
          const resp = await storeCollectedMessage(
            cfg,
            buildCollectedMessageEvent({
              platform: container.platform,
              chatId: container.chatId,
              threadId: container.threadId,
              messageId,
              content: event.content,
              senderId,
              senderName,
              senderUsername,
              collectorPubkey: undefined,
              direction: "inbound",
            }),
          );
          if (!resp.ok) {
            api.logger.warn(
              `memory-nomen: message.store inbound failed: ${resp.error?.message ?? "unknown error"}`,
            );
          }
        } catch (err) {
          api.logger.warn(
            `memory-nomen: message.store inbound failed: ${err instanceof Error ? err.message : err}`,
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

        api.logger.info(
          `memory-nomen: message_sent hook channel=${ctx?.channelId ?? "-"} conversation=${ctx?.conversationId ?? "-"} thread=${String(event?.metadata?.threadId ?? "-")} messageId=${String(event?.metadata?.messageId ?? "-")}`,
        );

        const container = resolveContainer({
          channelId: ctx?.channelId,
          conversationId: ctx?.conversationId,
          threadId: event?.metadata?.threadId,
        });
        const messageId = asString(event?.metadata?.messageId) ?? `sent:${Date.now()}`;
        const provider = asString(event?.metadata?.provider) ?? container.platform;
        const senderId = provider === "telegram" ? "openclaw" : "assistant";

        try {
          const resp = await storeCollectedMessage(
            cfg,
            buildCollectedMessageEvent({
              platform: container.platform,
              chatId: container.chatId,
              threadId: container.threadId,
              messageId,
              content: event.content,
              senderId,
              senderName: "Clarity",
              collectorPubkey: undefined,
              direction: "outbound",
            }),
          );
          if (!resp.ok) {
            api.logger.warn(
              `memory-nomen: message.store outbound failed: ${resp.error?.message ?? "unknown error"}`,
            );
          }
        } catch (err) {
          api.logger.warn(
            `memory-nomen: message.store outbound failed: ${err instanceof Error ? err.message : err}`,
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
              cfg.nsec,
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
              cfg.nsec,
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
                `[${r.visibility}] ${r.topic} (${r.match_type})`,
              );
              console.log(`  ${r.content?.split('\n')[0] || ''}`);
              console.log();
            }
          });
      },
      { commands: ["memory"] },
    );
  },
};

export default memoryNomenPlugin;
