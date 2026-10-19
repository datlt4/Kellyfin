// Inline SVG icons: text glyphs (▶ ↺ ⛶) sit off-centre and vary between fonts.
const PATHS = {
  play: "M8 5.14v13.72a1 1 0 0 0 1.52.85l11-6.86a1 1 0 0 0 0-1.7l-11-6.86A1 1 0 0 0 8 5.14z",
  pause: "M7 5h3.5v14H7zM13.5 5H17v14h-3.5z",
  back10: "M12 5V2L7 6l5 4V7a6 6 0 1 1-6 6H4a8 8 0 1 0 8-8z",
  fwd10: "M12 5V2l5 4-5 4V7a6 6 0 1 0 6 6h2a8 8 0 1 1-8-8z",
  next: "M6 6.5v11a.8.8 0 0 0 1.25.66L15 12.66V17.5h2v-11h-2v4.84L7.25 5.84A.8.8 0 0 0 6 6.5z",
  pip: "M3 5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v6h-2V5H5v14h6v2H5a2 2 0 0 1-2-2zm10 8h8v8h-8z",
  fullscreen: "M4 4h6v2H6v4H4zm10 0h6v6h-2V6h-4zM4 14h2v4h4v2H4zm14 0h2v6h-6v-2h4z",
  sun: "M12 7a5 5 0 1 1 0 10 5 5 0 0 1 0-10zm0-5 1 3h-2zm0 20-1-3h2zM2 12l3-1v2zm20 0-3 1v-2zM4.9 4.9l2.8 1.4-1.4 1.4zm14.2 14.2-2.8-1.4 1.4-1.4zM4.9 19.1l1.4-2.8 1.4 1.4zM19.1 4.9l-1.4 2.8-1.4-1.4z",
  volume: "M3 9v6h4l5 5V4L7 9zm13.5 3A4.5 4.5 0 0 0 14 8v8a4.5 4.5 0 0 0 2.5-4zM14 3.2v2.1a7 7 0 0 1 0 13.4v2.1a9 9 0 0 0 0-17.6z",
  left: "M15.4 7.4 14 6l-6 6 6 6 1.4-1.4-4.6-4.6z",
  right: "M8.6 16.6 10 18l6-6-6-6-1.4 1.4 4.6 4.6z",
  back: "M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20z",
  search: "M10 3a7 7 0 0 1 5.6 11.2l5.1 5.1-1.4 1.4-5.1-5.1A7 7 0 1 1 10 3zm0 2a5 5 0 1 0 0 10 5 5 0 0 0 0-10z",
  check: "M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z",
  heart: "M12 21s-7.5-4.6-9.6-9.3C.9 8.3 3 4.5 6.7 4.5c2.1 0 3.5 1.1 4.3 2.4.8-1.3 2.2-2.4 4.3-2.4 3.7 0 5.8 3.8 4.3 7.2C19.5 16.4 12 21 12 21z",
};

export type IconName = keyof typeof PATHS;

export default function Icon(props: { name: IconName; size?: number }) {
  return (
    <svg class="ico" viewBox="0 0 24 24" width={props.size ?? 22} height={props.size ?? 22} fill="currentColor" aria-hidden="true">
      <path d={PATHS[props.name]} />
    </svg>
  );
}
