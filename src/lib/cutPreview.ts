export type KeepSeg = { start: number; end: number };

export function keepContaining(keeps: KeepSeg[], t: number) {
  if (!keeps.length) return 0;
  for (let i = 0; i < keeps.length; i++) {
    if (t < keeps[i].end) return i;
  }
  return keeps.length - 1;
}

function fpsStep(fps: number) {
  return fps > 1 ? 1 / fps : 1 / 30;
}

function snapUp(t: number, step: number) {
  return Math.ceil((t - 1e-6) / step) * step;
}

/** Export re-encodes each kept span to constant-frame-rate video. Snap the
 * preview's keep edges the same way (first frame at/after the edge) so the
 * live player isn't aiming for a timestamp the codec can't land on. */
export function snapKeepsToFps(keeps: KeepSeg[], fps: number): KeepSeg[] {
  const step = fpsStep(fps);
  return keeps
    .map((k) => ({
      start: Math.max(0, snapUp(k.start, step)),
      end: Math.max(0, snapUp(k.end, step)),
    }))
    .filter((k) => k.end - k.start >= step * 0.5);
}

/** Last-resort audible-player seek. `overrun` is measured from the cut edge,
 * not the next keep's first frame, so a far-ahead target can't read as "late"
 * and a seek into the gap can't read as "early" forever. */
export function hardSeekNeeded(misses: number, overrun: number, tolerance: number) {
  const hardSeekAfter = Math.max(0.06, tolerance * 2);
  return misses > 2 && (overrun > hardSeekAfter || overrun < -tolerance);
}

const HAVE_CURRENT_DATA = 2;

type Opts = {
  getA: () => HTMLVideoElement | null;
  getB: () => HTMLVideoElement | null;
  getIdx: () => number;
  setIdx: (i: number) => void;
  getKeeps: () => KeepSeg[];
  getFps?: () => number;
  enabled: () => boolean;
  onTime: (t: number) => void;
  onActive?: (el: HTMLVideoElement) => void;
};

/** Ping-pong two video elements so cut skips don't seek the audible player. */
export function startCutPreview(opts: Opts) {
  let gen = 0;
  let prerollTo = -1;
  let swapping = false;
  let misses = 0;

  function els() {
    return { a: opts.getA(), b: opts.getB() };
  }
  function active() {
    const { a, b } = els();
    return opts.getIdx() === 0 ? a : b;
  }
  function standby() {
    const { a, b } = els();
    return opts.getIdx() === 0 ? b : a;
  }
  function fps() {
    return opts.getFps?.() || 30;
  }
  function step() {
    return fpsStep(fps());
  }
  function tolerance() {
    return Math.min(0.08, Math.max(0.018, step() * 0.75));
  }
  function keeps() {
    return snapKeepsToFps(opts.getKeeps(), fps());
  }

  function mark() {
    const { a, b } = els();
    if (!a || !b) return;
    const i = opts.getIdx();
    a.classList.toggle("standby", i !== 0);
    b.classList.toggle("standby", i === 0);
    a.muted = i !== 0;
    b.muted = i === 0;
  }

  function readyAt(el: HTMLVideoElement, t: number) {
    return (
      el.readyState >= HAVE_CURRENT_DATA &&
      !el.seeking &&
      Math.abs(el.currentTime - t) <= tolerance()
    );
  }

  function warm(st: HTMLVideoElement, t: number) {
    const onSeeked = () => {
      st.removeEventListener("seeked", onSeeked);
      if (prerollTo !== t || !st.paused) return;
      // Some browsers don't decode the target frame until the element has
      // played once. Play for a single frame, pause, then only re-seek when
      // the pause overshot the frame we actually wanted.
      void st
        .play()
        .then(() => {
          if (prerollTo !== t) return;
          st.pause();
          if (st.currentTime - t > tolerance()) {
            try {
              st.currentTime = t;
            } catch {
              /* src not ready */
            }
          }
        })
        .catch(() => {});
    };
    st.addEventListener("seeked", onSeeked);
  }

  function preroll() {
    const el = active();
    const st = standby();
    if (!el || !st || !opts.enabled()) return;
    const list = keeps();
    if (!list.length) return;
    const i = keepContaining(list, el.currentTime);
    const next = list[i + 1];
    if (!next) {
      prerollTo = -1;
      return;
    }
    if (prerollTo === next.start && (st.seeking || readyAt(st, next.start))) return;
    prerollTo = next.start;
    try {
      st.preload = "auto";
      // Decode the preroll frame at 1× so a 1.5×/2× preview doesn't overshoot
      // the target by a whole displayed frame before pause() lands.
      st.playbackRate = 1;
      warm(st, next.start);
      st.currentTime = next.start;
    } catch {
      /* src not ready */
    }
  }

  function swapTo(t: number, boundary = t) {
    const cur = active();
    const st = standby();
    if (!cur || !st || swapping || !opts.enabled()) return;
    if (readyAt(st, t)) {
      swapping = true;
      misses = 0;
      const prev = cur;
      prev.muted = true;
      st.playbackRate = prev.playbackRate;
      st.muted = false;
      void st.play();
      opts.setIdx(opts.getIdx() === 0 ? 1 : 0);
      mark();
      opts.onActive?.(st);
      prev.pause();
      prev.muted = true;
      prerollTo = -1;
      swapping = false;
      gen += 1;
      arm(st, gen);
      preroll();
      return;
    }

    // If preroll failed, don't let the audible player sail through the rest of
    // the cut. Give the standby a couple of frames, then hard-seek as a last
    // resort. `boundary` is the cut edge we already crossed; `t` may be far
    // ahead (the next keep's first frame), so lateness must be measured from
    // the boundary, not from the target.
    misses += 1;
    const overrun = cur.currentTime - boundary;
    if (hardSeekNeeded(misses, overrun, tolerance()) && !cur.seeking) {
      misses = 0;
      try {
        cur.currentTime = t;
      } catch {
        /* src not ready */
      }
    }
  }

  function tick(myGen: number) {
    if (myGen !== gen) return;
    const el = active();
    if (!el || el.paused) return;
    const t = el.currentTime;
    opts.onTime(t);
    if (opts.enabled()) {
      const list = keeps();
      if (list.length) {
        const i = keepContaining(list, t);
        const k = list[i];
        if (t < k.start - tolerance()) swapTo(k.start, k.start);
        else if (list[i + 1] && t >= k.end) swapTo(list[i + 1].start, k.end);
        else preroll();
      }
    }
    arm(el, myGen);
  }

  function arm(el: HTMLVideoElement, myGen: number) {
    const rvfc = (
      el as HTMLVideoElement & {
        requestVideoFrameCallback?: (cb: () => void) => number;
      }
    ).requestVideoFrameCallback;
    if (rvfc) rvfc.call(el, () => tick(myGen));
    else requestAnimationFrame(() => tick(myGen));
  }

  function onPlay(e: Event) {
    if (e.target !== active()) return;
    gen += 1;
    mark();
    preroll();
    const el = active();
    if (el) arm(el, gen);
  }

  const { a, b } = els();
  a?.addEventListener("play", onPlay);
  b?.addEventListener("play", onPlay);
  mark();

  return {
    destroy() {
      gen += 1;
      a?.removeEventListener("play", onPlay);
      b?.removeEventListener("play", onPlay);
    },
    preroll,
    mark,
  };
}
