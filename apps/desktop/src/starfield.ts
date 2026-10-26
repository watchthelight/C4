// <wtl> project: c4 | file: apps/desktop/src/starfield.ts | author: watchthelight </wtl>
// The uprooted.sh starfield behind the chrome: square-pixel stars on three depth layers that drift
// and twinkle, one in ten tinted sage, a rare shooting star drawn as fading pixels. It runs at
// 30 fps inside a 1 ms budget and freezes when the window is hidden, under reduced motion, and
// whenever a caller holds a freeze (a splitter drag, a streaming pane). Every star comes from a
// seeded generator, so a fixed seed gives the same sky twice, which the screenshot tests rely on.

export interface Layer {
  parallax: number;
  drift: number;
  size: number;
}

export const LAYERS: readonly Layer[] = [
  { parallax: 0.25, drift: 0.04, size: 1 },
  { parallax: 0.5, drift: 0.07, size: 1 },
  { parallax: 1.0, drift: 0.12, size: 2 },
];

export const STAR = [200, 210, 220] as const;
export const SAGE = [122, 170, 126] as const;
/** One star per this many square pixels. */
export const AREA_PER_STAR = 7000;
/** The most stars a canvas gets, whatever its size. */
export const MAX_STARS = 400;
/** Frames per second while running. */
export const FPS = 30;
/** How long one draw may take before the field thins itself. */
export const BUDGET_MS = 1;
/** Seconds between shooting stars: this plus up to nine more. */
const SHOT_MIN_MS = 7000;
const SHOT_SPREAD_MS = 9000;

export interface Star {
  x: number;
  y: number;
  layer: number;
  size: number;
  alpha: number;
  rate: number;
  sage: boolean;
}

interface Shot {
  x: number;
  y: number;
  dx: number;
  dy: number;
  life: number;
  decay: number;
  length: number;
}

/** A small deterministic generator (mulberry32). */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** How many stars a canvas of this size gets. */
export function starCount(width: number, height: number): number {
  return Math.min(MAX_STARS, Math.floor((width * height) / AREA_PER_STAR));
}

/** The stars for a canvas, from the generator. */
export function seedStars(width: number, height: number, next: () => number, count = starCount(width, height)): Star[] {
  const stars: Star[] = [];
  for (let i = 0; i < count; i += 1) {
    const layer = i % LAYERS.length;
    const big = layer === LAYERS.length - 1 && next() < 0.5;
    stars.push({
      x: next() * width,
      y: next() * height,
      layer,
      size: big ? 2 : 1,
      alpha: next() * 0.5 + 0.15,
      rate: next() * 0.003 + 0.001,
      sage: next() < 0.1,
    });
  }
  return stars;
}

export interface StarfieldOptions {
  /** The generator seed; the clock when absent. */
  seed?: number;
  /** Draw shooting stars. Off for screenshots. */
  shootingStars?: boolean;
  /** Overrides the media query, for tests. */
  reducedMotion?: boolean;
}

export interface Starfield {
  el: HTMLCanvasElement;
  /** Stop drawing until `thaw` is called with the same reason. */
  freeze(reason: string): void;
  thaw(reason: string): void;
  /** True while a frame loop is scheduled. */
  readonly running: boolean;
  /** The stars as seeded, for tests. */
  readonly stars: readonly Star[];
  /** Draw one frame now. */
  draw(now?: number): void;
  dispose(): void;
}

/** Mount a starfield canvas that fills `host`. The host needs `position: relative` or similar. */
export function createStarfield(host: HTMLElement, options: StarfieldOptions = {}): Starfield {
  const canvas = document.createElement("canvas");
  canvas.className = "stars";
  canvas.setAttribute("aria-hidden", "true");
  host.prepend(canvas);
  const ctx = canvas.getContext("2d");
  const reduced =
    options.reducedMotion ??
    (typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  const next = rng(options.seed ?? Date.now());
  const shooting = options.shootingStars ?? true;

  let width = 0;
  let height = 0;
  let stars: Star[] = [];
  let shots: Shot[] = [];
  let nextShot = 0;
  let raf = 0;
  let lastFrame = 0;
  let overBudget = 0;
  const holds = new Set<string>();
  let disposed = false;

  function resize(): void {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    width = host.clientWidth;
    height = host.clientHeight;
    canvas.width = Math.floor(width * dpr);
    canvas.height = Math.floor(height * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    ctx?.setTransform(dpr, 0, 0, dpr, 0, 0);
    stars = seedStars(width, height, next);
    shots = [];
    draw(lastFrame);
  }

  function shoot(now: number): void {
    const angle = Math.PI * (0.15 + next() * 0.2);
    const speed = 5 + next() * 5;
    shots.push({
      x: next() * width * 0.8,
      y: next() * height * 0.35,
      dx: Math.cos(angle) * speed,
      dy: Math.sin(angle) * speed,
      life: 1,
      decay: 0.012 + next() * 0.01,
      length: 8 + Math.floor(next() * 8),
    });
    nextShot = now + SHOT_MIN_MS + next() * SHOT_SPREAD_MS;
  }

  function draw(now: number): void {
    if (!ctx || width === 0 || height === 0) {
      return;
    }
    const started = performance.now();
    ctx.clearRect(0, 0, width, height);
    const time = now * 0.001;
    for (const s of stars) {
      const layer = LAYERS[s.layer];
      if (!layer) {
        continue;
      }
      if (!reduced) {
        s.x -= layer.drift;
        s.y += layer.drift * 0.6;
        if (s.x < -2) {
          s.x += width + 4;
        }
        if (s.y > height + 2) {
          s.y -= height + 4;
        }
      }
      const alpha = reduced ? s.alpha : Math.max(0, s.alpha + Math.sin(time * s.rate * 300 + s.x) * 0.08);
      const [r, g, b] = s.sage ? SAGE : STAR;
      ctx.fillStyle = `rgba(${r},${g},${b},${alpha.toFixed(3)})`;
      ctx.fillRect(Math.floor(s.x), Math.floor(s.y), s.size, s.size);
    }
    for (let i = shots.length - 1; i >= 0; i -= 1) {
      const m = shots[i];
      if (!m) {
        continue;
      }
      m.x += m.dx;
      m.y += m.dy;
      m.life -= m.decay;
      if (m.life <= 0 || m.x > width + 100 || m.y > height + 100) {
        shots.splice(i, 1);
        continue;
      }
      // The tail is a run of pixels that fade toward the back; no gradient, no antialiased line.
      const step = Math.hypot(m.dx, m.dy) / 2;
      const ux = m.dx / step;
      const uy = m.dy / step;
      for (let k = 0; k < m.length; k += 1) {
        const a = 0.55 * m.life * (1 - k / m.length);
        ctx.fillStyle = `rgba(${STAR[0]},${STAR[1]},${STAR[2]},${a.toFixed(3)})`;
        ctx.fillRect(Math.floor(m.x - ux * k * 2), Math.floor(m.y - uy * k * 2), 1, 1);
      }
      ctx.fillStyle = `rgba(230,236,240,${(0.8 * m.life).toFixed(3)})`;
      ctx.fillRect(Math.floor(m.x), Math.floor(m.y), 2, 2);
    }
    const took = performance.now() - started;
    if (took > BUDGET_MS * 2) {
      overBudget += 1;
      if (overBudget >= 3 && stars.length > 40) {
        // Too slow for this machine: thin the field by a quarter and keep going.
        stars = stars.slice(0, Math.floor(stars.length * 0.75));
        overBudget = 0;
      }
    } else {
      overBudget = 0;
    }
  }

  function frozen(): boolean {
    return disposed || reduced || holds.size > 0 || document.hidden;
  }

  function frame(now: number): void {
    raf = 0;
    if (frozen()) {
      return;
    }
    if (now - lastFrame >= 1000 / FPS - 1) {
      lastFrame = now;
      if (shooting) {
        if (nextShot === 0) {
          nextShot = now + 4000;
        } else if (now > nextShot) {
          shoot(now);
        }
      }
      draw(now);
    }
    raf = requestAnimationFrame(frame);
  }

  function start(): void {
    if (raf === 0 && !frozen()) {
      raf = requestAnimationFrame(frame);
    }
  }

  function stop(): void {
    if (raf !== 0) {
      cancelAnimationFrame(raf);
      raf = 0;
    }
  }

  function onVisibility(): void {
    if (document.hidden) {
      stop();
    } else {
      start();
    }
  }

  // A move to a monitor with different scaling changes the pixel ratio without changing the host's
  // size, so the resize observer stays quiet; a media query on the current ratio catches it.
  let ratioQuery: MediaQueryList | null = null;
  function watchRatio(): void {
    ratioQuery?.removeEventListener("change", onRatioChange);
    ratioQuery = null;
    if (typeof window.matchMedia === "function") {
      ratioQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      ratioQuery.addEventListener("change", onRatioChange);
    }
  }
  function onRatioChange(): void {
    resize();
    watchRatio();
  }

  const observer =
    typeof ResizeObserver === "undefined"
      ? null
      : new ResizeObserver(() => {
          resize();
        });
  observer?.observe(host);
  if (!observer) {
    window.addEventListener("resize", resize);
  }
  document.addEventListener("visibilitychange", onVisibility);
  watchRatio();
  resize();
  start();

  return {
    el: canvas,
    freeze(reason) {
      holds.add(reason);
      stop();
    },
    thaw(reason) {
      holds.delete(reason);
      start();
    },
    get running() {
      return raf !== 0;
    },
    get stars() {
      return stars;
    },
    draw(now = lastFrame) {
      draw(now);
    },
    dispose() {
      disposed = true;
      stop();
      observer?.disconnect();
      ratioQuery?.removeEventListener("change", onRatioChange);
      window.removeEventListener("resize", resize);
      document.removeEventListener("visibilitychange", onVisibility);
      canvas.remove();
    },
  };
}
