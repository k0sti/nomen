<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    createSimulation,
    updateSimulation,
    findConnections,
    particleCountForSize,
    DEFAULT_CONFIG,
    type SimulationState,
    type SimulationConfig,
  } from '../lib/particle-network';

  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D | null = null;
  let animationId: number = 0;
  let state: SimulationState | null = null;
  let config: SimulationConfig = { ...DEFAULT_CONFIG };
  let mouseX: number | null = null;
  let mouseY: number | null = null;
  let prefersReducedMotion = false;

  function init() {
    if (!canvas) return;
    ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    config = {
      ...DEFAULT_CONFIG,
      particleCount: particleCountForSize(rect.width, rect.height),
    };
    state = createSimulation(rect.width, rect.height, config);
  }

  function draw() {
    if (!ctx || !state) return;

    const w = state.width;
    const h = state.height;

    ctx.clearRect(0, 0, w, h);

    // Find connections
    const connections = findConnections(state.particles, config.connectionDistance);

    // Draw connections
    for (const conn of connections) {
      const a = state.particles[conn.a];
      const b = state.particles[conn.b];
      const alpha = conn.strength * 0.6;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.strokeStyle = `oklch(0.65 0.14 275 / ${alpha})`;
      ctx.lineWidth = conn.strength * 2;
      ctx.stroke();
    }

    // Draw particles
    for (const p of state.particles) {
      ctx.beginPath();
      ctx.arc(p.x, p.y, p.radius, 0, Math.PI * 2);
      ctx.fillStyle = `oklch(0.7 0.14 ${p.hue} / 0.8)`;
      ctx.fill();

      // Glow
      ctx.beginPath();
      ctx.arc(p.x, p.y, p.radius * 2.5, 0, Math.PI * 2);
      ctx.fillStyle = `oklch(0.6 0.14 ${p.hue} / 0.15)`;
      ctx.fill();
    }
  }

  function drawStatic() {
    if (!ctx || !state) return;
    // Draw one frame without animation for reduced motion
    draw();
  }

  function loop() {
    if (!state) return;
    updateSimulation(state, mouseX, mouseY, config);
    draw();
    animationId = requestAnimationFrame(loop);
  }

  function handleMouseMove(e: MouseEvent) {
    const rect = canvas.getBoundingClientRect();
    mouseX = e.clientX - rect.left;
    mouseY = e.clientY - rect.top;
  }

  function handleMouseLeave() {
    mouseX = null;
    mouseY = null;
  }

  function handleTouchMove(e: TouchEvent) {
    if (e.touches.length > 0) {
      const rect = canvas.getBoundingClientRect();
      mouseX = e.touches[0].clientX - rect.left;
      mouseY = e.touches[0].clientY - rect.top;
    }
  }

  function handleTouchEnd() {
    mouseX = null;
    mouseY = null;
  }

  function handleResize() {
    init();
    if (prefersReducedMotion) {
      drawStatic();
    }
  }

  onMount(() => {
    prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    init();

    if (prefersReducedMotion) {
      drawStatic();
    } else {
      loop();
    }
  });

  onDestroy(() => {
    if (animationId) cancelAnimationFrame(animationId);
  });
</script>

<svelte:window onresize={handleResize} />

<canvas
  bind:this={canvas}
  class="particle-canvas"
  onmousemove={handleMouseMove}
  onmouseleave={handleMouseLeave}
  ontouchmove={handleTouchMove}
  ontouchend={handleTouchEnd}
  aria-hidden="true"
></canvas>

<style>
  .particle-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: auto;
  }
</style>
