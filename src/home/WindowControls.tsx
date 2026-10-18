// Windows caption buttons for the custom 44 px title bar (UX guide: controls on the right).
// Glyphs from Segoe Fluent Icons, which ships with Windows 11.
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect, useState, type ReactElement } from 'react';
import { useT } from '../shared/i18n';
import './caption.css';

export function WindowControls({ maximizable = true }: { maximizable?: boolean }): ReactElement {
  const t = useT();
  // One handle per mount: getCurrentWindow() returns a new object each call, which would
  // re-run the effect (and re-subscribe) on every render.
  const [win] = useState(getCurrentWindow);
  const [max, setMax] = useState(false);
  useEffect(() => {
    void win.isMaximized().then(setMax);
    const off = win.onResized(() => void win.isMaximized().then(setMax));
    return () => void off.then((f) => f());
  }, [win]);
  return (
    <div className="hm-caption">
      <button type="button" className="hm-caption__btn" aria-label={t('window.minimize')} onClick={() => void win.minimize()}>{'\uE921'}</button>
      {maximizable && (
        <button type="button" className="hm-caption__btn" aria-label={t(max ? 'window.restore' : 'window.maximize')} onClick={() => void win.toggleMaximize()}>
          {max ? '\uE923' : '\uE922'}
        </button>
      )}
      <button type="button" className="hm-caption__btn hm-caption__btn--close" aria-label={t('window.close')} onClick={() => void win.close()}>{'\uE8BB'}</button>
    </div>
  );
}
