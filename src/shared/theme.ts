/**
 * Mirrors the effective appearance onto `<html data-theme>`; keeps following changes. Rust applies
 * `ui.theme` as the app appearance, so `prefers-color-scheme` already reflects a Light/Dark choice.
 */
export function applyTheme(): void {
  const mq = window.matchMedia('(prefers-color-scheme: dark)');
  const set = () => {
    document.documentElement.dataset.theme = mq.matches ? 'dark' : 'light';
  };
  set();
  mq.addEventListener('change', set);
}
