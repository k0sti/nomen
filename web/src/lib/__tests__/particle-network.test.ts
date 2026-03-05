import { describe, it, expect } from 'vitest';
import {
  createSimulation,
  createParticle,
  updateSimulation,
  findConnections,
  particleCountForSize,
  DEFAULT_CONFIG,
  type SimulationConfig,
} from '../particle-network';

const testConfig: SimulationConfig = {
  ...DEFAULT_CONFIG,
  particleCount: 10,
};

describe('createSimulation', () => {
  it('creates the correct number of particles', () => {
    const state = createSimulation(800, 600, testConfig);
    expect(state.particles).toHaveLength(10);
    expect(state.width).toBe(800);
    expect(state.height).toBe(600);
  });

  it('places particles within bounds', () => {
    const state = createSimulation(800, 600, testConfig);
    for (const p of state.particles) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(800);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeLessThanOrEqual(600);
    }
  });

  it('assigns hue in the blue-purple range', () => {
    const state = createSimulation(800, 600, testConfig);
    for (const p of state.particles) {
      expect(p.hue).toBeGreaterThanOrEqual(250);
      expect(p.hue).toBeLessThanOrEqual(310);
    }
  });
});

describe('createParticle', () => {
  it('respects radius bounds', () => {
    for (let i = 0; i < 50; i++) {
      const p = createParticle(800, 600, testConfig);
      expect(p.radius).toBeGreaterThanOrEqual(testConfig.minRadius);
      expect(p.radius).toBeLessThanOrEqual(testConfig.maxRadius);
    }
  });
});

describe('updateSimulation', () => {
  it('moves particles each tick', () => {
    const state = createSimulation(800, 600, testConfig);
    const initialPositions = state.particles.map((p) => ({ x: p.x, y: p.y }));

    updateSimulation(state, null, null, testConfig);

    let moved = false;
    for (let i = 0; i < state.particles.length; i++) {
      if (
        state.particles[i].x !== initialPositions[i].x ||
        state.particles[i].y !== initialPositions[i].y
      ) {
        moved = true;
        break;
      }
    }
    expect(moved).toBe(true);
  });

  it('wraps particles around edges', () => {
    const state = createSimulation(800, 600, testConfig);
    // Force a particle past the right edge
    state.particles[0].x = 850;
    state.particles[0].vx = 1;

    updateSimulation(state, null, null, testConfig);

    // Should have wrapped to left side
    expect(state.particles[0].x).toBeLessThan(0);
  });

  it('applies mouse attraction', () => {
    const config: SimulationConfig = { ...testConfig, mouseForce: 0.5, mouseRadius: 500 };
    const state = createSimulation(800, 600, config);

    // Place particle at center, mouse at right
    state.particles[0].x = 400;
    state.particles[0].y = 300;
    state.particles[0].vx = 0;
    state.particles[0].vy = 0;

    updateSimulation(state, 600, 300, config);

    // Particle should have moved rightward toward mouse
    expect(state.particles[0].vx).toBeGreaterThan(0);
  });
});

describe('findConnections', () => {
  it('finds connections between close particles', () => {
    const particles = [
      { x: 0, y: 0, vx: 0, vy: 0, radius: 2, hue: 270 },
      { x: 50, y: 0, vx: 0, vy: 0, radius: 2, hue: 270 },
      { x: 500, y: 500, vx: 0, vy: 0, radius: 2, hue: 270 },
    ];

    const connections = findConnections(particles, 100);
    expect(connections).toHaveLength(1);
    expect(connections[0].a).toBe(0);
    expect(connections[0].b).toBe(1);
  });

  it('returns strength based on distance', () => {
    const particles = [
      { x: 0, y: 0, vx: 0, vy: 0, radius: 2, hue: 270 },
      { x: 50, y: 0, vx: 0, vy: 0, radius: 2, hue: 270 },
    ];

    const connections = findConnections(particles, 100);
    expect(connections[0].strength).toBeCloseTo(0.5, 1);
  });

  it('returns empty array when no particles are close', () => {
    const particles = [
      { x: 0, y: 0, vx: 0, vy: 0, radius: 2, hue: 270 },
      { x: 500, y: 500, vx: 0, vy: 0, radius: 2, hue: 270 },
    ];

    const connections = findConnections(particles, 100);
    expect(connections).toHaveLength(0);
  });
});

describe('particleCountForSize', () => {
  it('returns ~80 for full HD viewport', () => {
    const count = particleCountForSize(1920, 1080);
    expect(count).toBe(80);
  });

  it('returns fewer particles for smaller viewports', () => {
    const count = particleCountForSize(375, 667);
    expect(count).toBeLessThan(80);
    expect(count).toBeGreaterThanOrEqual(30);
  });

  it('never goes below 30', () => {
    const count = particleCountForSize(100, 100);
    expect(count).toBe(30);
  });

  it('caps at 150', () => {
    const count = particleCountForSize(5000, 5000);
    expect(count).toBeLessThanOrEqual(150);
  });
});
