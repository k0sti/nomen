<script lang="ts">
  import ParticleBackground from './components/ParticleBackground.svelte';
</script>

<div class="landing">
  <div class="hero-bg">
    <ParticleBackground />
  </div>

  <div class="content">
    <header class="hero">
      <h1 class="title">Nomen</h1>
      <p class="tagline">Nostr Memory Network</p>
      <p class="subtitle">
        Collective, tiered memory for AI agents. From noise to knowledge —
        persistent, searchable, self-sovereign.
      </p>
      <div class="hero-actions">
        <a
          href="https://github.com/k0sti/nomen"
          target="_blank"
          rel="noopener noreferrer"
          class="btn-primary"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M9 19c-5 1.5-5-2.5-7-3m14 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 20 4.77 5.07 5.07 0 0 0 19.91 1S18.73.65 16 2.48a13.38 13.38 0 0 0-7 0C6.27.65 5.09 1 5.09 1A5.07 5.07 0 0 0 5 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 9 18.13V22" />
          </svg>
          Get Started
        </a>
        <a href="/memory/" class="btn-secondary">
          Open Dashboard
        </a>
      </div>
    </header>

    <section class="features">
      <div class="feature-card">
        <div class="feature-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
            <circle cx="9" cy="7" r="4" />
            <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
            <path d="M16 3.13a4 4 0 0 1 0 7.75" />
          </svg>
        </div>
        <h3>Collective & Tiered</h3>
        <p>Four memory tiers: public, group, personal, and internal. Personal memories are user-scoped — auditable by the user. Internal memories are agent-only. Named groups and npub sets, both hierarchical.</p>
      </div>

      <div class="feature-card">
        <div class="feature-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
            <line x1="8" y1="11" x2="14" y2="11" />
            <line x1="11" y1="8" x2="11" y2="14" />
          </svg>
        </div>
        <h3>Semantic Search</h3>
        <p>Hybrid vector + BM25 search finds memories by meaning. Confidence decay, importance scoring, and result aggregation for high-quality retrieval.</p>
      </div>

      <div class="feature-card">
        <div class="feature-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="6" cy="6" r="3" />
            <circle cx="18" cy="6" r="3" />
            <circle cx="6" cy="18" r="3" />
            <circle cx="18" cy="18" r="3" />
            <line x1="9" y1="6" x2="15" y2="6" />
            <line x1="6" y1="9" x2="6" y2="15" />
            <line x1="18" y1="9" x2="18" y2="15" />
            <line x1="9" y1="18" x2="15" y2="18" />
            <line x1="8.5" y1="8.5" x2="15.5" y2="15.5" />
          </svg>
        </div>
        <h3>Knowledge Graph</h3>
        <p>Entities automatically extracted and linked via graph edges — mentions, references, contradictions. SurrealDB-native graph queries.</p>
      </div>

      <div class="feature-card">
        <div class="feature-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M12 2L2 7l10 5 10-5-10-5z" />
            <path d="M2 17l10 5 10-5" />
            <path d="M2 12l10 5 10-5" />
          </svg>
        </div>
        <h3>Nostr-Native</h3>
        <p>Custom kind 31234 replaceable events on any Nostr relay. Memory travels with the agent's keypair — no vendor lock-in, no central database.</p>
      </div>

      <div class="feature-card">
        <div class="feature-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
          </svg>
        </div>
        <h3>Consolidation</h3>
        <p>Sleep-inspired pipeline: raw messages → LLM extraction → merge & dedup → named memories. Conflict detection, entity linking, automatic pruning.</p>
      </div>

      <div class="feature-card">
        <div class="feature-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
            <line x1="8" y1="21" x2="16" y2="21" />
            <line x1="12" y1="17" x2="12" y2="21" />
          </svg>
        </div>
        <h3>MCP & Context-VM</h3>
        <p>MCP server (stdio/HTTP) for agent frameworks. Context-VM for pure Nostr request/response — no MCP required. Web dashboard for operations.</p>
      </div>
    </section>

    <footer class="landing-footer">
      <p>Built on <a href="https://nostr.com" target="_blank" rel="noopener noreferrer">Nostr</a> · Powered by <a href="https://surrealdb.com" target="_blank" rel="noopener noreferrer">SurrealDB</a> · Open source</p>
    </footer>
  </div>
</div>

<style>
  .landing {
    position: relative;
    min-height: 100vh;
    min-height: 100dvh;
    background: oklch(0.06 0.02 265);
    overflow-x: hidden;
  }

  .hero-bg {
    position: fixed;
    inset: 0;
    z-index: 0;
  }

  .content {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-rows: 1fr auto auto;
    min-height: 100vh;
    min-height: 100dvh;
    padding: var(--space-6);
    pointer-events: none;
  }

  .content > * {
    pointer-events: auto;
  }

  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    gap: var(--space-4);
    padding-block: var(--space-10);
  }

  .title {
    font-size: clamp(3rem, 8vw, 6rem);
    font-weight: 700;
    letter-spacing: -0.03em;
    color: var(--color-text-primary);
    text-shadow:
      0 0 40px oklch(0.55 0.22 290 / 0.4),
      0 0 80px oklch(0.55 0.22 290 / 0.15);
    line-height: 1;
  }

  .tagline {
    font-size: clamp(1.25rem, 3vw, 1.75rem);
    color: var(--color-accent-light);
    font-weight: 400;
    max-width: 40ch;
  }

  .subtitle {
    font-size: var(--text-lg);
    color: var(--color-text-secondary);
    max-width: 50ch;
    line-height: 1.6;
  }

  .hero-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    justify-content: center;
    margin-top: var(--space-4);
  }

  .btn-primary,
  .btn-secondary {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-6);
    border-radius: 0.5rem;
    font-size: var(--text-base);
    font-weight: 500;
    text-decoration: none;
    min-height: 44px;
    min-width: 44px;
    transition: background 150ms, box-shadow 150ms;
    cursor: pointer;
    border: none;
    font-family: inherit;
  }

  .btn-primary {
    background: var(--color-accent);
    color: oklch(0.98 0.005 265);
  }

  .btn-primary:hover {
    background: var(--color-accent-hover);
    box-shadow: 0 0 20px oklch(0.55 0.22 290 / 0.3);
  }

  .btn-secondary {
    background: oklch(0.20 0.02 265 / 0.8);
    color: var(--color-text-primary);
    border: 1px solid var(--color-border);
  }

  .btn-secondary:hover {
    background: var(--color-surface-hover);
    border-color: var(--color-accent);
  }

  .features {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(280px, 100%), 1fr));
    gap: var(--space-6);
    max-width: 1200px;
    margin-inline: auto;
    width: 100%;
    padding-block: var(--space-10);
  }

  .feature-card {
    background: oklch(0.12 0.02 265 / 0.7);
    backdrop-filter: blur(12px);
    border: 1px solid oklch(0.25 0.02 265 / 0.5);
    border-radius: 0.75rem;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    transition: border-color 150ms, box-shadow 150ms;
  }

  .feature-card:hover {
    border-color: oklch(0.55 0.22 290 / 0.4);
    box-shadow: 0 0 24px oklch(0.55 0.22 290 / 0.08);
  }

  .feature-icon {
    color: var(--color-accent-light);
    width: 44px;
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .feature-card h3 {
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .feature-card p {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.6;
  }

  .landing-footer {
    text-align: center;
    padding-block: var(--space-8);
    font-size: var(--text-sm);
    color: var(--color-text-muted);
  }

  .landing-footer a {
    color: var(--color-accent-light);
    text-decoration: none;
  }

  .landing-footer a:hover {
    text-decoration: underline;
  }
</style>
