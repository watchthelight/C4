// <wtl> project: c4 | file: apps/desktop/tests/starfield.test.ts | author: watchthelight </wtl>
// The starfield's arithmetic (the parts that need no canvas): a fixed seed gives the same sky, the
// star count follows the area with a cap, a tenth of the stars are sage, and the field mounts and
// freezes without a drawing context, which jsdom does not provide.

import { describe, expect, it, vi } from "vitest";
import { AREA_PER_STAR, LAYERS, MAX_STARS, createStarfield, rng, seedStars, starCount } from "../src/starfield";

// jsdom has no 2D context and logs an error when asked for one; the field must cope with null.
vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);

describe("starfield", () => {
  it("counts stars by area with a cap", () => {
    expect(starCount(0, 0)).toBe(0);
    expect(starCount(700, 100)).toBe(10);
    expect(starCount(320, 900)).toBe(Math.floor((320 * 900) / AREA_PER_STAR));
    expect(starCount(4000, 4000)).toBe(MAX_STARS);
  });

  it("seeds the same sky from the same seed", () => {
    const a = seedStars(320, 800, rng(7));
    const b = seedStars(320, 800, rng(7));
    const c = seedStars(320, 800, rng(8));
    expect(a).toEqual(b);
    expect(a).not.toEqual(c);
    expect(a).toHaveLength(starCount(320, 800));
    for (const s of a) {
      expect(s.x).toBeGreaterThanOrEqual(0);
      expect(s.x).toBeLessThan(320);
      expect(s.y).toBeGreaterThanOrEqual(0);
      expect(s.y).toBeLessThan(800);
      expect(s.alpha).toBeGreaterThanOrEqual(0.15);
      expect(s.alpha).toBeLessThanOrEqual(0.65);
      expect(s.layer).toBeLessThan(LAYERS.length);
      expect([1, 2]).toContain(s.size);
    }
    expect(a.filter((s) => s.size === 2).every((s) => s.layer === LAYERS.length - 1)).toBe(true);
  });

  it("tints about one star in ten sage", () => {
    const stars = seedStars(2000, 2000, rng(42), 4000);
    const sage = stars.filter((s) => s.sage).length / stars.length;
    expect(sage).toBeGreaterThan(0.07);
    expect(sage).toBeLessThan(0.13);
  });

  it("mounts a canvas, holds freezes by reason and disposes", () => {
    const host = document.createElement("div");
    document.body.append(host);
    const field = createStarfield(host, { seed: 1, shootingStars: false, reducedMotion: true });
    expect(host.firstElementChild).toBe(field.el);
    expect(field.el.getAttribute("aria-hidden")).toBe("true");
    expect(field.running).toBe(false);
    expect(() => field.draw(0)).not.toThrow();
    field.freeze("drag");
    field.freeze("stream");
    field.thaw("drag");
    expect(field.running).toBe(false);
    field.thaw("stream");
    field.dispose();
    expect(host.querySelector("canvas")).toBeNull();
  });
});
