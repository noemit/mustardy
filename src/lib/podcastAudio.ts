type Graph = {
  ctx: AudioContext;
  comp: DynamicsCompressorNode;
  makeup: GainNode;
  gains: Map<HTMLVideoElement, GainNode>;
};

let graph: Graph | null = null;

function ensureGraph(): Graph | null {
  if (graph) return graph;
  try {
    const ctx = new AudioContext();
    const comp = ctx.createDynamicsCompressor();
    const makeup = ctx.createGain();
    comp.connect(makeup);
    makeup.connect(ctx.destination);
    graph = { ctx, comp, makeup, gains: new Map() };
    return graph;
  } catch {
    return null;
  }
}

function bothPaused() {
  const g = graph;
  if (!g) return true;
  for (const el of g.gains.keys()) if (!el.paused) return false;
  return true;
}

export function attachPodcastElement(el: HTMLVideoElement) {
  const g = ensureGraph();
  if (!g || g.gains.has(el)) return;
  try {
    const src = g.ctx.createMediaElementSource(el);
    const gain = g.ctx.createGain();
    src.connect(gain);
    gain.connect(g.comp);
    g.gains.set(el, gain);
    el.addEventListener("play", () => {
      if (g.ctx.state === "suspended") void g.ctx.resume();
    });
    el.addEventListener("pause", () => {
      if (bothPaused() && g.ctx.state === "running") void g.ctx.suspend();
    });
  } catch {
    /* already connected */
  }
}

export function setPodcastLive(el: HTMLVideoElement | null) {
  const g = graph;
  if (!g) return;
  for (const [node, gain] of g.gains) {
    gain.gain.value = node === el ? 1 : 0;
  }
}

/** Route video audio through a speech compressor. Approximate export dynaudnorm. */
export function setPodcastPreview(el: HTMLVideoElement, on: boolean, amount: number) {
  attachPodcastElement(el);
  const g = graph;
  if (!g) return;
  const a = Math.min(1, Math.max(0, amount));
  if (el.paused && bothPaused()) {
    if (g.ctx.state === "running") void g.ctx.suspend();
  } else if (g.ctx.state === "suspended") {
    void g.ctx.resume();
  }
  if (on) {
    g.comp.threshold.value = -20 - a * 14;
    g.comp.knee.value = 12;
    g.comp.ratio.value = 3 + a * 6;
    g.comp.attack.value = 0.005;
    g.comp.release.value = 0.16;
    g.makeup.gain.value = 1 + a * 1.6;
  } else {
    g.comp.threshold.value = 0;
    g.comp.ratio.value = 1;
    g.comp.attack.value = 0.003;
    g.comp.release.value = 0.05;
    g.makeup.gain.value = 1;
  }
}
