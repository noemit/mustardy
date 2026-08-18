export type FrameShot = { t: number; dataUrl: string };

export async function grabFrames(src: string, times: number[], size = 384): Promise<FrameShot[]> {
  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.preload = "auto";
  video.src = src;
  await wait(video, "loadeddata");

  const duration = video.duration || 0;
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx) return [];

  const unique = [...new Set(times.map((t) => clamp(t, 0.05, Math.max(0.05, duration - 0.08))))]
    .sort((a, b) => a - b)
    .slice(0, 8);

  const shots: FrameShot[] = [];
  for (const t of unique) {
    video.currentTime = t;
    await wait(video, "seeked");
    const scale = size / Math.max(video.videoWidth || size, video.videoHeight || size);
    canvas.width = Math.max(1, Math.round((video.videoWidth || size) * scale));
    canvas.height = Math.max(1, Math.round((video.videoHeight || size) * scale));
    ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
    shots.push({ t, dataUrl: canvas.toDataURL("image/jpeg", 0.72) });
  }
  video.removeAttribute("src");
  video.load();
  return shots;
}

export function sampleTimes(duration: number, playhead?: number) {
  if (!duration || duration <= 0) return playhead != null ? [playhead] : [];
  // Coarse pass: 5 evenly-spaced spots. The eyes only look closer (dense
  // frames around a cut point) when a plan needs precise times.
  const n = 5;
  const times: number[] = [];
  for (let i = 0; i < n; i++) times.push(((i + 0.5) / n) * duration);
  if (playhead != null) times.push(playhead);
  return times.filter((t) => t >= 0 && t <= duration);
}

function wait(el: HTMLVideoElement, ev: string) {
  return new Promise<void>((resolve, reject) => {
    const ok = () => {
      el.removeEventListener("error", fail);
      resolve();
    };
    const fail = () => {
      el.removeEventListener(ev, ok);
      reject(new Error("video frame failed"));
    };
    el.addEventListener(ev, ok, { once: true });
    el.addEventListener("error", fail, { once: true });
  });
}

function clamp(n: number, min: number, max: number) {
  return Math.min(max, Math.max(min, n));
}
