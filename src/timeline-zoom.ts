const clampScale = (scale: number) => Math.max(0.5, Math.min(4, scale));
export function zoomAtPointer(
  current: number,
  requested: number,
  scrollTop: number,
  pointerY: number,
  viewportHeight: number,
) {
  const scale = clampScale(requested);
  const y = Math.max(0, Math.min(viewportHeight, pointerY));
  const position = (scrollTop + y) / current;
  return {
    scale,
    scrollTop: Math.max(
      0,
      Math.min(1440 * scale + 1 - viewportHeight, position * scale - y),
    ),
  };
}
export function wheelZoomScale(
  current: number,
  delta: number,
  mode: number,
  viewport: number,
) {
  const pixels = delta * (mode === 1 ? 16 : mode === 2 ? viewport : 1);
  return clampScale(current * Math.exp(-pixels * 0.002));
}
