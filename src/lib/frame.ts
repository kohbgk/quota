/**
 * Runs `fn` once the page has rendered. Hidden windows don't get animation
 * frames (the overlay and settings windows start hidden), so a timer stands
 * in when frames aren't coming; whichever fires first wins.
 */
export function afterRender(fn: () => void, fallbackMs = 120): void {
  let done = false;
  const run = () => {
    if (done) return;
    done = true;
    fn();
  };
  requestAnimationFrame(() => requestAnimationFrame(run));
  setTimeout(run, fallbackMs);
}
