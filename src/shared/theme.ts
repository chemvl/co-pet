/** Mirrors macOS appearance onto `<html data-theme>`; keeps following changes. */
export function applyTheme(): void {
  const mq = window.matchMedia('(prefers-color-scheme: dark)');
  const set = () => {
    document.documentElement.dataset.theme = mq.matches ? 'dark' : 'light';
  };
  set();
  mq.addEventListener('change', set);
}
