// Nostr relay client — direct WebSocket communication with NIP-42 AUTH

import { type EventTemplate, type NostrEvent } from 'nostr-tools';
import type { Memory, Message, Group } from './api';

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
  private bufferedAuthChallenge: string | null = null;
  private _signer: Signer | null = null;
  private okHandlers = new Map<string, { resolve: (id: string) => void; reject: (err: Error) => void }>();
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 10;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private shouldReconnect = false;
  onConnectionChange?: (connected: boolean) => void;

  constructor(relay: string = 'wss://zooid.atlantislabs.space') {
    this.relay = relay;
  }

  get url(): string {
    return this.relay;
  }

  async connect(): Promise<void> {
    if (this.ws && this.connected) return;
    this.shouldReconnect = true;

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.ws?.close();
        reject(new Error('Connection timeout'));
      }, 10000);

      this.ws = new WebSocket(this.relay);

      this.ws.onopen = () => {
        this.connected = true;
        this.reconnectAttempts = 0;
        clearTimeout(timeout);
        this.onConnectionChange?.(true);
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
        const wasConnected = this.connected;
        this.connected = false;
        this.authenticated = false;
        if (wasConnected) {
          this.onConnectionChange?.(false);
        }
        this.scheduleReconnect();
      };
    });
  }

  private scheduleReconnect() {
    if (!this.shouldReconnect) return;
    if (this.reconnectAttempts >= this.maxReconnectAttempts) return;

    const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts), 30000);
    this.reconnectAttempts++;

    this.reconnectTimer = setTimeout(async () => {
      try {
        await this.connect();
        // Re-authenticate if we had a signer
        if (this._signer) {
          await this.authenticate(this._signer);
        }
        // Re-subscribe all active subscriptions
        for (const [subId] of this.subs) {
          // Subscriptions need to be re-established by the caller
          // The onConnectionChange callback enables this
        }
      } catch {
        // connect() will trigger another onclose -> scheduleReconnect
      }
    }, delay);
  }

  private authPromise: Promise<void> | null = null;

  async authenticate(signer: Signer): Promise<void> {
    if (this.authenticated) return;

    this._signer = signer;

    // Deduplicate: if auth is already in progress, wait for that
    if (this.authPromise) return this.authPromise;

    this.authPromise = this._doAuthenticate();
    try {
      await this.authPromise;
    } finally {
      this.authPromise = null;
    }
  }

  private async _doAuthenticate(): Promise<void> {
    // Ensure WebSocket is connected first
    if (!this.connected) {
      await this.connect();
    }

    // If AUTH challenge arrived before authenticate() was called, handle it now
    if (this.bufferedAuthChallenge) {
      const challenge = this.bufferedAuthChallenge;
      this.bufferedAuthChallenge = null;
      await this.handleAuth(challenge);
      return;
    }

    // Wait for auth challenge to arrive, or timeout (relay may not require auth)
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.pendingAuth = null;
        // Don't mark as authenticated on timeout — relay may still require it
        console.warn('[relay] AUTH challenge timeout — relay may not require auth');
        this.authenticated = true;
        resolve();
      }, 5000);

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
      const okHandler = this.okHandlers.get(eventId);
      if (okHandler) {
        this.okHandlers.delete(eventId);
        if (success) {
          okHandler.resolve(eventId);
        } else {
          okHandler.reject(new Error(msg[3] || 'Event rejected'));
        }
      }
    } else if (type === 'CLOSED') {
      const subId = msg[1] as string;
      const reason = (msg[2] as string) || '';
      const pending = this.pendingReqs.get(subId);
      if (pending) {
        clearTimeout(pending.timeout);
        this.pendingReqs.delete(subId);
        pending.reject(new Error(`Subscription closed: ${reason}`));
      }
    } else if (type === 'NOTICE') {
      console.warn('[relay notice]', msg[1]);
    }
  }

  private async handleAuth(challenge: string) {
    // If already authenticated, ignore subsequent AUTH challenges (zooid sends them repeatedly)
    if (this.authenticated) return;

    if (!this._signer) {
      // Signer not set yet — buffer challenge for when authenticate() is called
      this.bufferedAuthChallenge = challenge;
      return;
    }

    try {
      const authEvent: EventTemplate = {
        kind: 22242,
        created_at: Math.floor(Date.now() / 1000),
        tags: [
          ['relay', this.relay],
          ['challenge', challenge],
        ],
        content: '',
      };

      const signed = await this._signer.signEvent(authEvent);
      this.send(JSON.stringify(['AUTH', signed]));
      this.authenticated = true;
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

  private requestOnce(filter: Record<string, any>, timeoutMs = 15000): Promise<NostrEvent[]> {
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

  private async request(filter: Record<string, any>, timeoutMs = 15000): Promise<NostrEvent[]> {
    try {
      return await this.requestOnce(filter, timeoutMs);
    } catch (err: any) {
      // Auto-retry once on auth-required: re-authenticate and try again
      if (err.message?.includes('auth-required') && this._signer) {
        this.authenticated = false;
        this.authPromise = null; // allow fresh auth attempt
        await this.authenticate(this._signer);
        // Wait for relay to process the AUTH response
        await new Promise(r => setTimeout(r, 300));
        return await this.requestOnce(filter, timeoutMs);
      }
      throw err;
    }
  }

  private async publish(event: NostrEvent, timeoutMs = 10000): Promise<string> {
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.okHandlers.delete(event.id);
        reject(new Error('Publish timeout'));
      }, timeoutMs);

      this.okHandlers.set(event.id, {
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

  // ── Forward a pre-signed event to this relay ─────────────────
  async publishEvent(event: NostrEvent): Promise<string> {
    return this.publish(event);
  }

  // ── Memory operations ─────────────────────────────────────────

  async listMemories(_pubkey?: string): Promise<Memory[]> {
    // Fetch all memory events on this relay (not filtered by author)
    const events = await this.request({
      kinds: [30078],
      limit: 500,
    });

    return events
      .filter((e) => this.isMemoryEvent(e))
      .map((e) => this.parseMemory(e));
  }

  subscribeMemories(_pubkey: string | undefined, callback: (m: Memory) => void): Subscription {
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
        { kinds: [30078], since: Math.floor(Date.now() / 1000) },
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

  async deleteMemory(eventId: string, signer: Signer, dTag?: string): Promise<void> {
    const pubkey = await signer.getPublicKey();
    const tags: string[][] = [['e', eventId]];
    // Kind 30078 are addressable events — use a-tag for proper NIP-09 deletion
    if (dTag) {
      tags.push(['a', `30078:${pubkey}:${dTag}`]);
    }

    const event: EventTemplate = {
      kind: 5,
      created_at: Math.floor(Date.now() / 1000),
      tags,
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
    // Step 1: Query kind 0 profiles on this relay
    const kind0Events = await this.request({ kinds: [0], limit: 500 });
    const profileMap = new Map<string, { meta: Record<string, any>; created_at: number }>();
    for (const e of kind0Events) {
      try {
        profileMap.set(e.pubkey, {
          meta: JSON.parse(e.content),
          created_at: e.created_at,
        });
      } catch { /* skip malformed */ }
    }

    // Return only pubkeys that have a kind 0 profile on this relay
    const profiles: { pubkey: string; meta: Record<string, any>; created_at: number }[] = [];
    for (const [pubkey, p] of profileMap) {
      profiles.push({ pubkey, meta: p.meta, created_at: p.created_at });
    }

    return profiles;
  }

  // ── Disconnect ────────────────────────────────────────────────

  disconnect() {
    this.shouldReconnect = false;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    // Close all subscriptions
    for (const subId of this.subs.keys()) {
      this.send(JSON.stringify(['CLOSE', subId]));
    }
    this.subs.clear();
    this.pendingReqs.clear();
    this.okHandlers.clear();
    this.bufferedAuthChallenge = null;
    this._signer = null;
    this.ws?.close();
    this.ws = null;
    this.connected = false;
    this.authenticated = false;
    this.onConnectionChange?.(false);
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
