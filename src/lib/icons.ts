// Small inline SVG icons (16px grid, 1.5px strokes, currentColor).

const svg = (body: string, extra = "") =>
  `<svg class="icon" viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" ${extra}>${body}</svg>`;

export const icons = {
  refresh: svg('<path d="M13.25 8a5.25 5.25 0 1 1-1.54-3.71"/><path d="M13.25 2.75v2.9h-2.9"/>'),
  sliders: svg('<path d="M2.75 5h6.5M12.25 5h1M2.75 11h1M6.75 11h6.5"/><circle cx="10.75" cy="5" r="1.5"/><circle cx="5.25" cy="11" r="1.5"/>'),
  close: svg('<path d="M4.25 4.25l7.5 7.5M11.75 4.25l-7.5 7.5"/>'),
  alert: svg('<path d="M8 2.25 14.25 13H1.75Z"/><path d="M8 6.5v2.75M8 11.25v.01"/>'),
  info: svg('<circle cx="8" cy="8" r="5.75"/><path d="M8 7.25v3.5M8 5.25v.01"/>'),
  lock: svg('<rect x="3.75" y="7" width="8.5" height="6.25" rx="1.5"/><path d="M5.75 7V5.5a2.25 2.25 0 0 1 4.5 0V7"/>'),
  link: svg('<path d="M6.75 9.25l2.5-2.5"/><path d="M8.6 4.4l.9-.9a2.47 2.47 0 0 1 3.5 3.5l-.9.9"/><path d="M7.4 11.6l-.9.9a2.47 2.47 0 0 1-3.5-3.5l.9-.9"/>'),
  download: svg('<path d="M8 2.75v7M5.25 7.25 8 10l2.75-2.75"/><path d="M3 13h10"/>'),
  user: svg('<circle cx="8" cy="5.5" r="2.5"/><path d="M3.5 13.25c.6-2.2 2.35-3.35 4.5-3.35s3.9 1.15 4.5 3.35"/>'),
  clock: svg('<circle cx="8" cy="8" r="5.75"/><path d="M8 5v3.1l2 1.4"/>'),
  check: svg('<path d="M3.5 8.5 6.5 11.25 12.5 4.75"/>'),
  keyboard: svg('<rect x="1.75" y="4" width="12.5" height="8" rx="1.75"/><path d="M4.5 7h.01M7 7h.01M9.5 7h.01M12 7h.01M5 9.5h6"/>'),
};

/** Quota's mark: a usage ring with a "Q" tail. */
export const logo = `<svg class="logo" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
  <defs><linearGradient id="qg" x1="0.2" y1="0" x2="0.8" y2="1"><stop offset="0" stop-color="#8BF7CF"/><stop offset="1" stop-color="#25C987"/></linearGradient>
  <mask id="qm"><rect width="16" height="16" fill="#fff"/><line x1="10" y1="10" x2="12.6" y2="12.6" stroke="#000" stroke-width="3.6" stroke-linecap="round"/></mask></defs>
  <g mask="url(#qm)" fill="none" stroke-width="2.4">
    <circle cx="7.75" cy="7.75" r="5.1" stroke="currentColor" stroke-opacity="0.18"/>
    <circle cx="7.75" cy="7.75" r="5.1" stroke="url(#qg)" stroke-linecap="round" stroke-dasharray="22.4 32" transform="rotate(-90 7.75 7.75)"/>
  </g>
  <line x1="10.1" y1="10.1" x2="12.6" y2="12.6" stroke="currentColor" stroke-width="2.3" stroke-linecap="round"/>
</svg>`;
