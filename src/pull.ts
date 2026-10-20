import { onCleanup } from "solid-js";

const PULL = 90;

/** Pull down at the top of a page to reload it (the apps have no reload button); el is the indicator. */
export function pullToRefresh(el: HTMLElement) {
  let y0 = -1, x0 = 0, dy = 0;
  const show = (d: number) => {
    el.style.opacity = String(Math.min(1, d / PULL));
    el.style.transform = `translateY(${Math.min(d, PULL) / 2}px) rotate(${d * 3}deg)`;
  };
  const start = (e: TouchEvent) => {
    y0 = -1;
    dy = 0;
    if (scrollY > 0 || e.touches.length !== 1) return;
    // Not while an inner list (menus, pickers) is scrolled.
    for (let n = e.target as Element | null; n; n = n.parentElement) if (n.scrollTop > 0) return;
    y0 = e.touches[0].clientY;
    x0 = e.touches[0].clientX;
  };
  const move = (e: TouchEvent) => {
    if (y0 < 0) return;
    const t = e.touches[0];
    dy = t.clientY - y0;
    // Scrolling down the page or swiping a shelf sideways.
    if (Math.abs(t.clientX - x0) > dy) y0 = -1, dy = 0;
    show(Math.max(0, dy));
  };
  const end = () => {
    if (y0 >= 0 && dy > PULL) location.reload();
    y0 = -1;
    show(0);
  };
  const opts = { passive: true };
  document.addEventListener("touchstart", start, opts);
  document.addEventListener("touchmove", move, opts);
  document.addEventListener("touchend", end, opts);
  document.addEventListener("touchcancel", end, opts);
  onCleanup(() => {
    document.removeEventListener("touchstart", start);
    document.removeEventListener("touchmove", move);
    document.removeEventListener("touchend", end);
    document.removeEventListener("touchcancel", end);
  });
}
