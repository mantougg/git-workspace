/**
 * Frontend frame-time measurement channel (global constraint §2: 前端渲染预算).
 *
 * Samples rAF frame durations via `performance.now()` — independent of the
 * T-07 Rust benchmark. Live stats are exposed on `window.__gwPerf[label]`
 * for manual inspection, and frames slower than `SLOW_FRAME_MS` are logged
 * as warnings (throttled).
 */

export interface FrameStats {
  /** Sampled frame count (ring buffer, last ~1200 frames). */
  frames: number;
  avgMs: number;
  p95Ms: number;
  maxMs: number;
}

declare global {
  interface Window {
    __gwPerf?: Record<string, () => FrameStats>;
  }
}

const SLOW_FRAME_MS = 50;
const MAX_SAMPLES = 1200;

export function computeFrameStats(durations: number[]): FrameStats {
  if (durations.length === 0) {
    return { frames: 0, avgMs: 0, p95Ms: 0, maxMs: 0 };
  }
  const sorted = [...durations].sort((a, b) => a - b);
  const sum = sorted.reduce((acc, d) => acc + d, 0);
  const p95 = sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))];
  return {
    frames: sorted.length,
    avgMs: Math.round((sum / sorted.length) * 10) / 10,
    p95Ms: Math.round(p95 * 10) / 10,
    maxMs: Math.round(sorted[sorted.length - 1] * 10) / 10,
  };
}

/**
 * Start sampling frame times under `label`. Returns a stop function that
 * freezes and returns the final stats.
 */
export function startFrameMeter(label: string): () => FrameStats {
  const durations: number[] = [];
  const stats = () => computeFrameStats(durations);

  window.__gwPerf = window.__gwPerf ?? {};
  window.__gwPerf[label] = stats;

  let running = true;
  let last = performance.now();
  let slowCount = 0;

  function tick(now: number) {
    if (!running) return;
    const d = now - last;
    last = now;
    if (durations.push(d) > MAX_SAMPLES) durations.shift();
    if (d > SLOW_FRAME_MS) {
      slowCount += 1;
      // Throttle slow-frame warnings: first few, then one per ~second of them.
      if (slowCount <= 5 || slowCount % 60 === 0) {
        console.warn(
          `[perf:${label}] slow frame ${d.toFixed(1)}ms (> ${SLOW_FRAME_MS}ms)`,
        );
      }
    }
    requestAnimationFrame(tick);
  }
  requestAnimationFrame(tick);

  return () => {
    running = false;
    if (window.__gwPerf) delete window.__gwPerf[label];
    return stats();
  };
}
