// Nostr relay client — direct WebSocket communication with NIP-42 AUTH

import { finalizeEvent, type EventTemplate, type NostrEvent } from 'nostr-tools';
import type { Memory, Message, Group } from './api';
import { fetchProfilesBatch } from './nostr';

type Signer = {
  getPublicKey(): Promise<string>;
  signEvent(event: EventTemplate): Promise<NostrEvent>;
};

export interface Subscription {
  close(): void;
}

interface PendingReq {
  subId: string;
  events: NostrEvent[];
  resolve: (events: NostrEvent[]) => void;
  reject: (err: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
  streaming?: (event: NostrEvent) => void;
}

export class NomenRelay {
  private ws: WebSocket | null = null;
  private relay: string;
  private pendingAuth: {
    resolve: () => void;
    reject: (err: Error) => void;
  } | null = null;
  private pendingReqs = new Map<string, PendingReq>();
  private subs = new Map<string, (event: NostrEvent) => void>();
  private authenticated = false;
  private messageQueue: string[] = [];
  private connected = false;

  constructor(relay: string = 'wss://zooid.atlantislabs.space') {
    this.relay = relay;
  }

  get url(): string {
    return this.relay;
  }

  async connect(): Promise<void> {
    if (this.ws && this.connected) return;

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.ws?.close();
        reject(new Error('Connection timeout'));
      }, 10000);

      this.ws = new WebSocket(this.relay);

      this.ws.onopen = () => {
        this.connected = true;
        clearTimeout(timeout);
        // Flush queued messages
        for (const msg of this.messageQueue) {
          this.ws!.send(msg);
        }
        this.messageQueue = [];
        resolve();
      };

      this.ws.onmessage = (evt) => {
        this.handleMessage(evt.data);
      };

      this.ws.onerror = () => {
        clearTimeout(timeout);
        reject(new Error('WebSocket error'));
      };

      this.ws.onclose = () => {
        this.connected = false;
        this.authenticated = false;
      };
    });
  }

  async authenticate(signer: Signer): Promise<void> {
    // Auth happens via challenge from relay — wait for it or it may have already happened
    if (this.authenticated) return;

    // Store signer for when AUTH challenge arrives
    (this as any)._signer = signer;

    // Wait for auth to complete (challenge may arrive at any time after connect)
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        // If no AUTH challenge received, relay may not require it
        this.authenticated = true;
        resolve();
      }, 3000);

      this.pendingAuth = {
        resolve: () => {
          clearTimeout(timeout);
          this.authenticated = true;
          resolve();
        },
        reject: (err) => {
          clearTimeout(timeout);
          reject(err);
        },
      };
    });
  }

  private async handleMessage(data: string) {
    let msg: any[];
    try {
      msg = JSON.parse(data);
    } catch {
      return;
    }

    const type = msg[0];

    if (type === 'AUTH') {
      await this.handleAuth(msg[1]);
    } else if (type === 'EVENT') {
      const subId = msg[1] as string;
      const event = msg[2] as NostrEvent;

      // Check pending one-shot requests
      const pending = this.pendingReqs.get(subId);
      if (pending) {
        if (pending.streaming) {
          pending.streaming(event);
        }
        pending.events.push(event);
      }

      // Check live subscriptions
      const handler = this.subs.get(subId);
      if (handler) {
        handler(event);
      }
    } else if (type === 'EOSE') {
      const subId = msg[1] as string;
      const pending = this.pendingReqs.get(subId);
      if (pending) {
        clearTimeout(pending.timeout);
        this.pendingReqs.delete(subId);
        // Close the subscription for one-shot requests
        this.send(JSON.stringify(['CLOSE', subId]));
        pending.resolve(pending.events);
      }
    } else if (type === 'OK') {
      // Event publish confirmation — msg[1] is event id, msg[2] is success boolean
      const eventId = msg[1] as string;
      const success = msg[2] as boolean;
      const okHandler = (this as any)._okHandlers?.get(eventId);
      if (okHandler) {
        (this as any)._okHandlers.delete(eventId);
        if (success) {
          okHandler.resolve(eventId);
        } else {
          okHandler.reject(new Error(msg[3] || 'Event rejected'));
        }
      }
    } else if (type === 'NOTICE') {
      console.warn('[relay notice]', msg[1]);
    }
  }

  private async handleAuth(challenge: string) {
    const signer = (this as any)._signer as Signer | undefined;
    if (!signer) {
      console.warn('AUTH challenge received but no signer available');
      return;
    }

    try {
      const pubkey = await signer.getPublicKey();
      const authEvent: EventTemplate = {
        kind: 22242,
        created_at: Math.floor(Date.now() / 1000),
        tags: [
          ['relay', this.relay],
          ['challenge', challenge],
        ],
        content: '',
      };

      const signed = await signer.signEvent(authEvent);
      this.send(JSON.stringify(['AUTH', signed]));
      this.pendingAuth?.resolve();
      this.pendingAuth = null;
    } catch (err: any) {
      this.pendingAuth?.reject(err);
      this.pendingAuth = null;
    }
  }

  private send(msg: string) {
    if (this.ws && this.connected) {
      this.ws.send(msg);
    } else {
      this.messageQueue.push(msg);
    }
  }

  private genSubId(): string {
    return crypto.randomUUID().slice(0, 8);
  }

  private request(filter: Record<string, any>, timeoutMs = 15000): Promise<NostrEvent[]> {
    return new Promise((resolve, reject) => {
      const subId = this.genSubId();
      const timeout = setTimeout(() => {
        this.pendingReqs.delete(subId);
        this.send(JSON.stringify(['CLOSE', subId]));
        reject(new Error('Request timeout'));
      }, timeoutMs);

      this.pendingReqs.set(subId, {
        subId,
        events: [],
        resolve,
        reject,
        timeout,
      });

      this.send(JSON.stringify(['REQ', subId, filter]));
    });
  }

  private async publish(event: NostrEvent, timeoutMs = 10000): Promise<string> {
    return new Promise((resolve, reject) => {
      if (!(this as any)._okHandlers) {
        (this as any)._okHandlers = new Map();
      }
      const timeout = setTimeout(() => {
        (this as any)._okHandlers.delete(event.id);
        reject(new Error('Publish timeout'));
      }, timeoutMs);

      (this as any)._okHandlers.set(event.id, {
        resolve: (id: string) => {
          clearTimeout(timeout);
          resolve(id);
        },
        reject: (err: Error) => {
          clearTimeout(timeout);
          reject(err);
        },
      });

      this.send(JSON.stringify(['EVENT', event]));
    });
  }

  // ── Memory operations ─────────────────────────────────────────

  async listMemories(pubkey: string): Promise<Memory[]> {
    const events = await this.request({
      kinds: [30078],
      authors: [pubkey],
      limit: 500,
    });

    return events
      .filter((e) => this.isMemoryEvent(e))
      .map((e) => this.parseMemory(e));
  }

  subscribeMemories(pubkey: string, callback: (m: Memory) => void): Subscription {
    const subId = this.genSubId();

    this.subs.set(subId, (event: NostrEvent) => {
      if (this.isMemoryEvent(event)) {
        callback(this.parseMemory(event));
      }
    });

    this.send(
      JSON.stringify([
        'REQ',
        subId,
        { kinds: [30078], authors: [pubkey], since: Math.floor(Date.now() / 1000) },
      ])
    );

    return {
      close: () => {
        this.subs.delete(subId);
        this.send(JSON.stringify(['CLOSE', subId]));
      },
    };
  }

  async storeMemory(
    topic: string,
    summary: string,
    detail: string,
    tier: string,
    signer: Signer
  ): Promise<string> {
    const dTag = `snow:memory:${tier}:${topic}`;
    const content = JSON.stringify({
      summary,
      detail,
      topic,
      confidence: 0.8,
      created_at: Math.floor(Date.now() / 1000),
    });

    const event: EventTemplate = {
      kind: 30078,
      created_at: Math.floor(Date.now() / 1000),
      tags: [
        ['d', dTag],
        ['snow:tier', tier],
        ['snow:topic', topic],
        ['snow:confidence', '0.8'],
        ['snow:version', '1'],
      ],
      content,
    };

    const signed = await signer.signEvent(event);
    return this.publish(signed);
  }

  async deleteMemory(eventId: string, signer: Signer): Promise<void> {
    const event: EventTemplate = {
      kind: 5,
      created_at: Math.floor(Date.now() / 1000),
      tags: [['e', eventId]],
      content: 'Memory deleted via Nomen UI',
    };

    const signed = await signer.signEvent(event);
    await this.publish(signed);
  }

  // ── Group messages (kind 9) ───────────────────────────────────

  async getGroupMessages(groupId: string, limit: number = 50): Promise<Message[]> {
    const events = await this.request({
      kinds: [9],
      '#h': [groupId],
      limit,
    });

    return events.map((e) => this.parseMessage(e, groupId));
  }

  // ── NIP-29 groups (kind 39000) ────────────────────────────────

  async listGroups(): Promise<Group[]> {
    const events = await this.request({
      kinds: [39000],
      limit: 100,
    });

    return events.map((e) => this.parseGroup(e));
  }

  // ── Generic NIP-78 app data ──────────────────────────────────

  async fetchAppData(pubkey: string, dTag: string): Promise<NostrEvent | null> {
    const events = await this.request({
      kinds: [30078],
      authors: [pubkey],
      '#d': [dTag],
      limit: 1,
    });
    return events[0] || null;
  }

  async publishAppData(dTag: string, content: string, signer: Signer): Promise<string> {
    const event: EventTemplate = {
      kind: 30078,
      created_at: Math.floor(Date.now() / 1000),
      tags: [['d', dTag]],
      content,
    };
    const signed = await signer.signEvent(event);
    return this.publish(signed);
  }

  // ── Profiles (kind 0) ───────────────────────────────────────

  async listProfiles(): Promise<{ pubkey: string; meta: Record<string, any>; created_at: number }[]> {
    // Step 1: Get all unique pubkeys from events on this relay
    // (zooid doesn't store kind 0, so we discover members from their activity)
    const events = await this.request({ limit: 500 });
    const pubkeyTimestamps = new Map<string, number>();
    for (const e of events) {
      const existing = pubkeyTimestamps.get(e.pubkey) || 0;
      if (e.created_at > existing) {
        pubkeyTimestamps.set(e.pubkey, e.created_at);
      }
    }

    const pubkeys = [...pubkeyTimestamps.keys()];
    if (pubkeys.length === 0) return [];

    // Step 2: Fetch kind 0 profiles from public relays
    const metaMap = await fetchProfilesBatch(pubkeys);

    const profiles: { pubkey: string; meta: Record<string, any>; created_at: number }[] = [];
    for (const pubkey of pubkeys) {
      profiles.push({
        pubkey,
        meta: metaMap.get(pubkey) || {},
        created_at: pubkeyTimestamps.get(pubkey) || 0,
      });
    }

    return profiles;
  }

  // ── Disconnect ────────────────────────────────────────────────

  disconnect() {
    // Close all subscriptions
    for (const subId of this.subs.keys()) {
      this.send(JSON.stringify(['CLOSE', subId]));
    }
    this.subs.clear();
    this.pendingReqs.clear();
    this.ws?.close();
    this.ws = null;
    this.connected = false;
    this.authenticated = false;
  }

  // ── Parsers ───────────────────────────────────────────────────

  private isMemoryEvent(event: NostrEvent): boolean {
    const dTag = event.tags.find((t) => t[0] === 'd')?.[1] || '';
    return dTag.startsWith('snow:memory:') || dTag.startsWith('snowclaw:memory:');
  }

  private parseMemory(event: NostrEvent): Memory {
    const dTag = event.tags.find((t) => t[0] === 'd')?.[1] || '';
    const tierTag = event.tags.find((t) => t[0] === 'snow:tier')?.[1] || '';
    const topicTag = event.tags.find((t) => t[0] === 'snow:topic')?.[1] || '';
    const confTag = event.tags.find((t) => t[0] === 'snow:confidence')?.[1] || '';
    const modelTag = event.tags.find((t) => t[0] === 'snow:model')?.[1] || '';
    const versionTag = event.tags.find((t) => t[0] === 'snow:version')?.[1] || '1';
    const sourceTag = event.tags.find((t) => t[0] === 'snow:source')?.[1] || '';

    // Parse content JSON
    let summary = '';
    let detail = '';
    let topic = topicTag;
    let confidence = parseFloat(confTag) || 0.8;

    try {
      const parsed = JSON.parse(event.content);
      summary = parsed.summary || '';
      detail = parsed.detail || '';
      if (parsed.topic && !topic) topic = parsed.topic;
      if (parsed.confidence) confidence = parsed.confidence;
    } catch {
      // If content isn't JSON, use raw content as detail
      detail = event.content;
    }

    // Extract tier and scope from d-tag if not in tags
    let tier = tierTag;
    let scope = '';
    if (!tier) {
      const parts = dTag.replace('snow:memory:', '').replace('snowclaw:memory:', '').split(':');
      if (parts[0] === 'core' || parts[0] === 'public') {
        tier = 'public';
      } else if (parts[0] === 'group') {
        tier = 'group';
        scope = parts[1] || '';
      } else if (parts[0] === 'npub') {
        tier = 'private';
        scope = parts[1] || '';
      } else if (parts[0] === 'lesson') {
        tier = 'public';
      } else {
        tier = 'public';
      }
    }

    // Extract topic from d-tag if not in tags
    if (!topic) {
      // d-tag format: snow:memory:<tier>:<topic> or snow:memory:<topic>
      const prefix = dTag.startsWith('snowclaw:memory:') ? 'snowclaw:memory:' : 'snow:memory:';
      const rest = dTag.slice(prefix.length);
      const colonIdx = rest.indexOf(':');
      topic = colonIdx >= 0 ? rest.slice(colonIdx + 1) : rest;
    }

    return {
      id: event.id,
      topic,
      summary,
      detail,
      tier,
      scope,
      confidence,
      model: modelTag,
      version: versionTag,
      source: sourceTag,
      created_at: new Date(event.created_at * 1000).toISOString(),
      d_tag: dTag,
    };
  }

  private parseMessage(event: NostrEvent, groupId: string): Message {
    const channel = event.tags.find((t) => t[0] === 'h')?.[1] || groupId;

    return {
      id: event.id,
      source: 'nostr',
      sender: event.pubkey.slice(0, 12) + '...',
      channel,
      content: event.content,
      metadata: '',
      consolidated: false,
      created_at: new Date(event.created_at * 1000).toISOString(),
    };
  }

  private parseGroup(event: NostrEvent): Group {
    const dTag = event.tags.find((t) => t[0] === 'd')?.[1] || '';
    const name = event.tags.find((t) => t[0] === 'name')?.[1] || dTag;
    const members = event.tags.filter((t) => t[0] === 'p').map((t) => t[1]);

    return {
      id: dTag,
      name,
      members,
      nostr_group: dTag,
      relay: this.relay,
      created_at: new Date(event.created_at * 1000).toISOString(),
    };
  }
}
