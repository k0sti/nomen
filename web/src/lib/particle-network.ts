// Particle network simulation — pure functions, no DOM dependency

export interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  radius: number;
  hue: number; // OKLCH hue in degrees
}

export interface Connection {
  a: number; // index into particles array
  b: number;
  strength: number; // 0..1 based on distance
}

export interface SimulationState {
  particles: Particle[];
  width: number;
  height: number;
}

export interface SimulationConfig {
  particleCount: number;
  connectionDistance: number;
  speed: number;
  mouseRadius: number;
  mouseForce: number;
  minRadius: number;
  maxRadius: number;
}

export const DEFAULT_CONFIG: SimulationConfig = {
  particleCount: 80,
  connectionDistance: 150,
  speed: 0.3,
  mouseRadius: 200,
  mouseForce: 0.02,
  minRadius: 1.5,
  maxRadius: 3,
};

/** Create initial simulation state with random particles */
export function createSimulation(
  width: number,
  height: number,
  config: SimulationConfig = DEFAULT_CONFIG,
): SimulationState {
  const particles: Particle[] = [];
  for (let i = 0; i < config.particleCount; i++) {
    particles.push(createParticle(width, height, config));
  }
  return { particles, width, height };
}

/** Create a single particle with random position/velocity */
export function createParticle(
  width: number,
  height: number,
  config: SimulationConfig,
): Particle {
  const angle = Math.random() * Math.PI * 2;
  const speed = config.speed * (0.5 + Math.random() * 0.5);
  return {
    x: Math.random() * width,
    y: Math.random() * height,
    vx: Math.cos(angle) * speed,
    vy: Math.sin(angle) * speed,
    radius: config.minRadius + Math.random() * (config.maxRadius - config.minRadius),
    hue: 250 + Math.random() * 60, // 250-310: blue to purple range
  };
}

/** Advance simulation by one tick. Mutates state in place for performance. */
export function updateSimulation(
  state: SimulationState,
  mouseX: number | null,
  mouseY: number | null,
  config: SimulationConfig = DEFAULT_CONFIG,
): void {
  const { particles, width, height } = state;

  for (let i = 0; i < particles.length; i++) {
    const p = particles[i];

    // Mouse attraction
    if (mouseX !== null && mouseY !== null) {
      const dx = mouseX - p.x;
      const dy = mouseY - p.y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < config.mouseRadius && dist > 1) {
        const force = config.mouseForce * (1 - dist / config.mouseRadius);
        p.vx += (dx / dist) * force;
        p.vy += (dy / dist) * force;
      }
    }

    // Damping to prevent runaway velocities
    p.vx *= 0.99;
    p.vy *= 0.99;

    // Clamp speed
    const speed = Math.sqrt(p.vx * p.vx + p.vy * p.vy);
    const maxSpeed = config.speed * 2;
    if (speed > maxSpeed) {
      p.vx = (p.vx / speed) * maxSpeed;
      p.vy = (p.vy / speed) * maxSpeed;
    }

    // Move
    p.x += p.vx;
    p.y += p.vy;

    // Enforce minimum speed so particles never stop
    const minSpeed = config.speed * 0.5;
    if (speed < minSpeed && speed > 0.001) {
      p.vx = (p.vx / speed) * minSpeed;
      p.vy = (p.vy / speed) * minSpeed;
    } else if (speed <= 0.001) {
      const angle = Math.random() * Math.PI * 2;
      p.vx = Math.cos(angle) * minSpeed;
      p.vy = Math.sin(angle) * minSpeed;
    }

    // Wrap around edges with padding
    const pad = 20;
    if (p.x < -pad) p.x = width + pad;
    else if (p.x > width + pad) p.x = -pad;
    if (p.y < -pad) p.y = height + pad;
    else if (p.y > height + pad) p.y = -pad;
  }
}

/** Find all connections between nearby particles */
export function findConnections(
  particles: Particle[],
  maxDistance: number,
): Connection[] {
  const connections: Connection[] = [];
  const maxDist2 = maxDistance * maxDistance;

  for (let i = 0; i < particles.length; i++) {
    for (let j = i + 1; j < particles.length; j++) {
      const dx = particles[i].x - particles[j].x;
      const dy = particles[i].y - particles[j].y;
      const dist2 = dx * dx + dy * dy;
      if (dist2 < maxDist2) {
        const dist = Math.sqrt(dist2);
        connections.push({
          a: i,
          b: j,
          strength: 1 - dist / maxDistance,
        });
      }
    }
  }

  return connections;
}

/** Responsive particle count based on viewport area */
export function particleCountForSize(width: number, height: number): number {
  const area = width * height;
  // ~80 particles at 1920x1080, scale linearly with area, min 30
  const count = Math.round((area / (1920 * 1080)) * 80);
  return Math.max(30, Math.min(count, 150));
}
