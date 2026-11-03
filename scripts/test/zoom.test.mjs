import assert from "node:assert/strict";
import { test } from "node:test";
import { createZoomEase, easeStep, wheelFactor, WHEEL_STEP_MAX } from "../../src/lib/zoom.ts";

test("a large wheel delta is clamped, and lines and pages are scaled to pixels", () => {
  const notch = wheelFactor({ deltaY: 100, deltaMode: 0 }, 800);
  assert.equal(notch, Math.exp(-WHEEL_STEP_MAX * 0.01));
  assert.equal(wheelFactor({ deltaY: 5000, deltaMode: 0 }, 800), notch);
  assert.equal(wheelFactor({ deltaY: -5000, deltaMode: 0 }, 800), 1 / notch);
  // A small pinch delta is used as it is.
  assert.equal(wheelFactor({ deltaY: -3, deltaMode: 0 }, 800), Math.exp(0.03));
  // Three lines are 48 pixels, clamped; a page is the page height, clamped.
  assert.equal(wheelFactor({ deltaY: 3, deltaMode: 1 }, 800), notch);
  assert.equal(wheelFactor({ deltaY: 1, deltaMode: 2 }, 800), notch);
});

test("easing closes on the target without overshoot and ends exactly on it", () => {
  for (const [from, to] of [
    [1, 8],
    [8, 1],
    [1, 1.3],
  ]) {
    let z = from;
    let frames = 0;
    while (z !== to && frames < 200) {
      const next = easeStep(z, to, 16);
      assert.ok(from < to ? next > z && next <= to : next < z && next >= to, `${from}->${to}`);
      z = next;
      frames++;
    }
    assert.equal(z, to);
    assert.ok(frames > 3 && frames < 100, `${frames} frames`);
  }
  // A long frame (a stall) doesn't jump past the target.
  assert.ok(easeStep(1, 4, 10_000) <= 4);
  assert.equal(easeStep(2, 2, 16), 2);
});

/** A frame loop run by hand, 16 ms a frame. */
function setup(initial = 1) {
  let zoom = initial;
  let id = 0;
  let queued = null;
  let now = 1000;
  const applied = [];
  const ease = createZoomEase({
    get: () => zoom,
    min: 1,
    max: 16,
    apply: (z, x, y) => {
      zoom = z;
      applied.push([z, x, y]);
    },
    request: (cb) => {
      queued = cb;
      return ++id;
    },
    cancel: () => {
      queued = null;
    },
  });
  const frame = () => {
    now += 16;
    const cb = queued;
    queued = null;
    cb?.(now);
    return cb !== undefined && cb !== null;
  };
  const run = () => {
    let n = 0;
    while (frame() && n < 500) n++;
    return n;
  };
  return { ease, applied, frame, run, zoom: () => zoom, busy: () => queued !== null };
}

test("a few big asks become many small steps towards the sum of the asks", () => {
  const s = setup();
  s.ease.by(1.5, 10, 20);
  s.ease.by(1.5, 10, 20);
  s.ease.by(1.5, 30, 40);
  const frames = s.run();
  assert.ok(frames > 5, `${frames} frames`);
  assert.ok(Math.abs(s.zoom() - 1.5 ** 3) < 1e-9);
  const zooms = s.applied.map(([z]) => z);
  for (let i = 1; i < zooms.length; i++) assert.ok(zooms[i] >= zooms[i - 1]);
  // No frame zooms by as much as one ask did.
  const biggest = Math.max(...zooms.map((z, i) => z / (zooms[i - 1] ?? 1)));
  assert.ok(biggest < 1.5, String(biggest));
  // It keeps to the point of the latest ask.
  assert.deepEqual(s.applied.at(-1).slice(1), [30, 40]);
});

test("the limits are reached exactly, and asking past them does nothing more", () => {
  const s = setup();
  for (let i = 0; i < 10; i++) s.ease.by(2, 0, 0);
  s.run();
  assert.equal(s.zoom(), 16);
  assert.ok(s.applied.every(([z]) => z <= 16));
  for (let i = 0; i < 10; i++) s.ease.by(0.5, 0, 0);
  s.run();
  assert.equal(s.zoom(), 1);
  assert.ok(s.applied.every(([z]) => z >= 1));
  assert.ok(!s.busy());
});

test("stop drops what is under way; a new ask starts from where the zoom is", () => {
  const s = setup();
  s.ease.by(4, 0, 0);
  s.frame();
  s.frame();
  s.ease.stop();
  assert.ok(!s.busy());
  const at = s.zoom();
  assert.ok(at > 1 && at < 4);
  s.ease.by(1.1, 0, 0);
  s.run();
  assert.ok(Math.abs(s.zoom() - at * 1.1) < 1e-9);
});

test("an instant change (reduced motion) applies at once and ends any easing", () => {
  const s = setup();
  s.ease.by(4, 0, 0);
  s.ease.to(2, 5, 6, true);
  assert.equal(s.zoom(), 2);
  assert.ok(!s.busy());
  s.ease.to(40, 0, 0, true);
  assert.equal(s.zoom(), 16);
});
