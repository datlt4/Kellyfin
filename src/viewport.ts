// On-screen keyboards cover the page instead of shrinking it (iOS, Android edge-to-edge):
// reserve room below the content for them and bring the focused field into view.
const vv = window.visualViewport;
if (vv) {
  const fit = () =>
    document.documentElement.style.setProperty("--kb", `${Math.max(0, Math.round(innerHeight - vv.height - vv.offsetTop))}px`);
  vv.addEventListener("resize", fit);
  vv.addEventListener("scroll", fit);
  fit();
}

document.addEventListener("focusin", (e) => {
  const t = e.target as HTMLElement;
  if (!t.matches?.("input:not([type=range]):not([type=checkbox]), textarea")) return;
  // After the keyboard has finished sliding in.
  setTimeout(() => t.scrollIntoView({ block: "center", behavior: "smooth" }), 350);
});
