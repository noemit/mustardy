import { describe, expect, it } from "vitest";
import { guessTitle, hookFromCaption, localPlan } from "./planner";
import type { VideoInfo } from "../types";

const video: VideoInfo = {
  path: "/tmp/demo.mp4",
  url: "asset://demo.mp4",
  name: "demo.mp4",
  duration: 60,
  width: 1280,
  height: 720,
  fps: 30,
  hasAudio: true,
};

describe("localPlan", () => {
  it("queues silence trims only when silences are named", () => {
    const silences = [{ start: 5, end: 8 }];
    expect(localPlan("trim the silences", video, silences)).toHaveLength(1);
    // a bare "trim …" is not a silence request
    expect(localPlan("trim something else", video, silences)).toHaveLength(0);
  });

  it("maps fx vocabulary to typed changes", () => {
    expect(localPlan("panic shake", video, [])[0].type).toBe("shake");
    expect(localPlan("slow-mo that", video, [])[0].type).toBe("slow");
    expect(localPlan("smash to text \"NOPE\"", video, [])[0].type).toBe("textcard");
  });

  it("pulls quoted copy for overlays and titles", () => {
    const overlay = localPlan('overlay that says "WATCH THIS"', video, []);
    expect(overlay[0].text).toBe("WATCH THIS");
  });
});

describe("title helpers", () => {
  it("skips camera-default filenames", () => {
    expect(guessTitle("IMG_1234.mov")).toBe("");
    expect(guessTitle("untitled.mp4")).toBe("");
    expect(guessTitle("my_cool-demo.mp4")).toBe("MY COOL DEMO");
  });

  it("strips articles and trailing periods from captions", () => {
    expect(hookFromCaption("A cat on the desk.")).toBe("CAT ON THE DESK");
  });
});
