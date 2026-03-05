import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { NomenRelay } from '../src/lib/relay';
import type { NostrEvent, EventTemplate } from 'nostr-tools';

// ── Mock WebSocket ──────────────────────────────────────────────

type WSHandler = (evt: { data: string }) => void;

class MockWebSocket {
  static instances: MockWebSocket[] = [];

  url: string;
  onopen: (() => void) | null = null;
  onmessage: WSHandler | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  sent: string[] = [];
  closed = false;

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
    // Auto-open on next tick
    queueMicrotask(() => this.onopen?.());
  }

  send(data: string) {
    this.sent.push(data);
  }

  close() {
    this.closed = true;
    this.onclose?.();
  }

  // Test helper: simulate server sending a message
  serverSend(msg: any[]) {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
}

// ── Mock signer ─────────────────────────────────────────────────

function createMockSigner(pubkey = 'aabbccdd') {
  return {
    getPublicKey: vi.fn().mockResolvedValue(pubkey),
    signEvent: vi.fn().mockImplementation(async (event: EventTemplate) => ({
      ...event,
      id: 'signed-event-id',
      pubkey,
      sig: 'fakesig',
    })),
  };
}

// ── Mock crypto.randomUUID ──────────────────────────────────────

let uuidCounter = 0;

// ── Setup ───────────────────────────────────────────────────────

beforeEach(() => {
  MockWebSocket.instances = [];
  uuidCounter = 0;
  vi.stubGlobal('WebSocket', MockWebSocket);
  vi.stubGlobal('crypto', {
    randomUUID: () => `${String(++uuidCounter).padStart(8, '0')}-0000-0000-0000-000000000000`,
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

// ── Helpers ─────────────────────────────────────────────────────

function latestWs(): MockWebSocket {
  return MockWebSocket.instances[MockWebSocket.instances.length - 1];
}

function makeKind0Event(pubkey: string, name: string): NostrEvent {
  return {
    id: `id-${name}`,
    pubkey,
    created_at: Math.floor(Date.now() / 1000),
    kind: 0,
    tags: [],
    content: JSON.stringify({ name, display_name: name }),
    sig: 'fakesig',
  } as NostrEvent;
}

// ── Tests ───────────────────────────────────────────────────────

describe('NomenRelay', () => {
  describe('connect + authenticate with zooid AUTH flow', () => {
    it('handles AUTH challenge that arrives before authenticate() is called', async () => {
      const relay = new NomenRelay('wss://test.relay');
      const signer = createMockSigner();

      const connectPromise = relay.connect();
      await connectPromise;

      const ws = latestWs();

      // Server sends AUTH challenge immediately (before authenticate is called)
      ws.serverSend(['AUTH', 'challenge-123']);

      // Now call authenticate — should pick up the buffered challenge
      await relay.authenticate(signer);

      // Should have signed and sent AUTH response
      expect(signer.signEvent).toHaveBeenCalledOnce();
      const signedArg = signer.signEvent.mock.calls[0][0];
      expect(signedArg.kind).toBe(22242);
      expect(signedArg.tags).toContainEqual(['challenge', 'challenge-123']);

      // Should have sent AUTH message
      const authMsg = ws.sent.find((m) => JSON.parse(m)[0] === 'AUTH');
      expect(authMsg).toBeDefined();

      relay.disconnect();
    });

    it('handles AUTH challenge that arrives after authenticate() is called', async () => {
      const relay = new NomenRelay('wss://test.relay');
      const signer = createMockSigner();

      await relay.connect();
      const ws = latestWs();

      // Start authenticate (will wait for challenge)
      const authPromise = relay.authenticate(signer);

      // Server sends AUTH challenge
      ws.serverSend(['AUTH', 'challenge-456']);

      await authPromise;

      expect(signer.signEvent).toHaveBeenCalledOnce();
      const authMsg = ws.sent.find((m) => JSON.parse(m)[0] === 'AUTH');
      expect(authMsg).toBeDefined();

      relay.disconnect();
    });

    it('times out gracefully when no AUTH challenge is sent', async () => {
      vi.useFakeTimers();
      const relay = new NomenRelay('wss://test.relay');
      const signer = createMockSigner();

      await relay.connect();

      const authPromise = relay.authenticate(signer);

      // Advance past the 3s auth timeout
      vi.advanceTimersByTime(3100);

      await authPromise;

      // Should not have signed anything
      expect(signer.signEvent).not.toHaveBeenCalled();

      relay.disconnect();
      vi.useRealTimers();
    });
  });

  describe('request flow', () => {
    it('full auth + REQ + EVENT + EOSE flow returns events', async () => {
      const relay = new NomenRelay('wss://test.relay');
      const signer = createMockSigner('pk1');

      await relay.connect();
      const ws = latestWs();

      // AUTH challenge arrives before authenticate
      ws.serverSend(['AUTH', 'ch1']);
      await relay.authenticate(signer);

      // Request kind 0 profiles
      const profilesPromise = relay.listProfiles();

      // Find the REQ message for kind 0
      const reqMsg = ws.sent.find((m) => {
        const parsed = JSON.parse(m);
        return parsed[0] === 'REQ' && parsed[2]?.kinds?.[0] === 0;
      });
      expect(reqMsg).toBeDefined();
      const subId = JSON.parse(reqMsg!)[1];

      // Server sends events then EOSE
      const testProfile = makeKind0Event('pk-alice', 'Alice');
      ws.serverSend(['EVENT', subId, testProfile]);
      ws.serverSend(['EOSE', subId]);

      // listProfiles will also issue a second REQ (activity events) — respond to it
      await vi.waitFor(() => {
        const activityReq = ws.sent.find((m) => {
          const parsed = JSON.parse(m);
          return parsed[0] === 'REQ' && !parsed[2]?.kinds;
        });
        expect(activityReq).toBeDefined();
      });

      const activityReq = ws.sent.find((m) => {
        const parsed = JSON.parse(m);
        return parsed[0] === 'REQ' && !parsed[2]?.kinds;
      });
      const activitySubId = JSON.parse(activityReq!)[1];
      ws.serverSend(['EOSE', activitySubId]);

      const profiles = await profilesPromise;
      expect(profiles.length).toBeGreaterThanOrEqual(1);
      expect(profiles.find((p) => p.pubkey === 'pk-alice')).toBeDefined();
      expect(profiles.find((p) => p.meta.name === 'Alice')).toBeDefined();

      relay.disconnect();
    });
  });

  describe('CLOSED message handling', () => {
    it('rejects pending request when relay sends CLOSED', async () => {
      const relay = new NomenRelay('wss://test.relay');
      const signer = createMockSigner();

      await relay.connect();
      const ws = latestWs();
      ws.serverSend(['AUTH', 'ch1']);
      await relay.authenticate(signer);

      // Access private request method via listMemories
      const memoriesPromise = relay.listMemories('somepubkey');

      // Find the REQ
      const reqMsg = ws.sent.find((m) => {
        const parsed = JSON.parse(m);
        return parsed[0] === 'REQ';
      });
      const subId = JSON.parse(reqMsg!)[1];

      // Server closes with auth-required
      ws.serverSend(['CLOSED', subId, 'auth-required: need AUTH']);

      await expect(memoriesPromise).rejects.toThrow('Subscription closed: auth-required');

      relay.disconnect();
    });
  });

  describe('multiple REQs after auth', () => {
    it('handles concurrent requests with different subIds', async () => {
      const relay = new NomenRelay('wss://test.relay');
      const signer = createMockSigner();

      await relay.connect();
      const ws = latestWs();
      ws.serverSend(['AUTH', 'ch1']);
      await relay.authenticate(signer);

      // Fire two requests concurrently
      const memoriesPromise = relay.listMemories('pk1');
      const groupsPromise = relay.listGroups();

      // Find both REQs
      const reqs = ws.sent
        .filter((m) => JSON.parse(m)[0] === 'REQ')
        .map((m) => JSON.parse(m));

      expect(reqs.length).toBe(2);
      const [sub1, sub2] = [reqs[0][1], reqs[1][1]];
      expect(sub1).not.toBe(sub2);

      // Respond to both
      ws.serverSend(['EOSE', sub1]);
      ws.serverSend(['EOSE', sub2]);

      const memories = await memoriesPromise;
      const groups = await groupsPromise;

      expect(memories).toEqual([]);
      expect(groups).toEqual([]);

      relay.disconnect();
    });
  });

  describe('disconnect', () => {
    it('cleans up state on disconnect', async () => {
      const relay = new NomenRelay('wss://test.relay');
      await relay.connect();
      const ws = latestWs();

      relay.disconnect();

      expect(ws.closed).toBe(true);
    });
  });
});
