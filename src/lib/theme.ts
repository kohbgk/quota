import type { Theme } from "./types";

const media = window.matchMedia("(prefers-color-scheme: dark)");
let current: Theme = "system";

function apply() {
  const resolved = current === "system" ? (media.matches ? "dark" : "light") : current;
  document.documentElement.dataset.theme = resolved;
  document.documentElement.style.colorScheme = resolved;
}

media.addEventListener("change", apply);

export function setTheme(theme: Theme) {
  current = theme;
  apply();
}
