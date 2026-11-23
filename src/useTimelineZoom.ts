import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { wheelZoomScale, zoomAtPointer } from "./timeline-zoom";
export function useTimelineZoom(enabled: boolean) {
  const [element, setElement] = useState<HTMLDivElement | null>(null);
  const elementRef = useRef<HTMLDivElement | null>(null);
  const viewportRef = useCallback((node: HTMLDivElement | null) => {
    elementRef.current = node;
    setElement(node);
  }, []);
  const scaleRef = useRef(1);
  const pendingScroll = useRef<number | null>(null);
  const [scale, setScale] = useState(1);
  const apply = useCallback(
    (requested: number, pointerY: number) => {
      if (!element) return;
      const next = zoomAtPointer(
        scaleRef.current,
        requested,
        pendingScroll.current ?? element.scrollTop,
        pointerY,
        element.clientHeight,
      );
      if (next.scale === scaleRef.current) return;
      scaleRef.current = next.scale;
      pendingScroll.current = next.scrollTop;
      setScale(next.scale);
    },
    [element],
  );
  useLayoutEffect(() => {
    if (!enabled || !element) return;
    const wheel = (event: WheelEvent) => {
      if (!event.ctrlKey) return;
      event.preventDefault();
      apply(
        wheelZoomScale(
          scaleRef.current,
          event.deltaY,
          event.deltaMode,
          element.clientHeight,
        ),
        event.clientY - element.getBoundingClientRect().top,
      );
    };
    // React's delegated wheel listener is passive; the native listener must be
    // non-passive to stop WebView's page zoom for this timeline gesture.
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [enabled, apply, element]);
  useLayoutEffect(() => {
    const viewport = elementRef.current;
    if (enabled && viewport && pendingScroll.current !== null) {
      viewport.scrollTop = pendingScroll.current;
      pendingScroll.current = null;
    }
  }, [enabled, scale, element]);
  const reset = () => {
    if (element) apply(1, element.clientHeight / 2);
  };
  return { viewportRef, scale, reset };
}
