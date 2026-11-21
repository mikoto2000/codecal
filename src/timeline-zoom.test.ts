import { describe, it, expect } from "vitest";
import { zoomAtPointer, wheelZoomScale } from "./timeline-zoom";
describe("timeline zoom", () => {
  it("keeps the same timeline instant under the pointer", () => {
    expect(zoomAtPointer(1, 2, 480, 120, 400)).toEqual({
      scale: 2,
      scrollTop: 1080,
    });
    expect(zoomAtPointer(2, 1, 1080, 120, 400)).toEqual({
      scale: 1,
      scrollTop: 480,
    });
  });
  it("clamps zoom and scroll position to the visible timeline", () => {
    expect(zoomAtPointer(1, 100, 0, 0, 400).scale).toBe(4);
    expect(zoomAtPointer(1, 0.01, 0, 0, 400)).toEqual({
      scale: 0.5,
      scrollTop: 0,
    });
    expect(zoomAtPointer(1, 0.5, 1041, 0, 400).scrollTop).toBe(321);
  });
  it("normalizes pixel, line and page wheel deltas and reverses direction", () => {
    expect(wheelZoomScale(1, -1, 1, 400)).toBeCloseTo(
      wheelZoomScale(1, -16, 0, 400),
    );
    expect(wheelZoomScale(1, -1, 2, 400)).toBeCloseTo(
      wheelZoomScale(1, -400, 0, 400),
    );
    const larger = wheelZoomScale(1, -120, 0, 400);
    expect(larger).toBeGreaterThan(1);
    expect(wheelZoomScale(larger, 120, 0, 400)).toBeCloseTo(1);
    expect(wheelZoomScale(1, 0, 0, 400)).toBe(1);
  });
});
