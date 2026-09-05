import { useCallback, useEffect, useRef, type PointerEvent as ReactPointerEvent } from "react";

const SCROLLBAR_HIDE_DELAY_MS = 700;
const SCROLLBAR_HOT_ZONE_PX = 22;

export function useAutoHideScrollbar<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const hideTimer = useRef<number | null>(null);

  const clearHideTimer = useCallback(() => {
    if (hideTimer.current !== null) window.clearTimeout(hideTimer.current);
    hideTimer.current = null;
  }, []);

  const onScroll = useCallback(() => {
    const element = ref.current;
    if (!element) return;
    clearHideTimer();
    element.classList.add("is-scroll-active");
    hideTimer.current = window.setTimeout(() => {
      element.classList.remove("is-scroll-active");
      hideTimer.current = null;
    }, SCROLLBAR_HIDE_DELAY_MS);
  }, [clearHideTimer]);

  const onPointerMove = useCallback((event: ReactPointerEvent<T>) => {
    const element = event.currentTarget;
    const bounds = element.getBoundingClientRect();
    const isScrollable = element.scrollHeight > element.clientHeight + 1;
    element.classList.toggle("is-scrollbar-near", isScrollable && bounds.right - event.clientX <= SCROLLBAR_HOT_ZONE_PX);
  }, []);

  const onPointerLeave = useCallback((event: ReactPointerEvent<T>) => {
    event.currentTarget.classList.remove("is-scrollbar-near");
  }, []);

  useEffect(() => () => clearHideTimer(), [clearHideTimer]);

  return { ref, onScroll, onPointerMove, onPointerLeave };
}
