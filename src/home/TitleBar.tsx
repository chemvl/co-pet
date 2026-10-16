// 44 px title bar over the native traffic lights (TitleBarStyle::Overlay): wordmark, page
// name, the centred search button that opens the command palette (⌘K) and the "Local only"
// status pill. Empty areas drag the window.
import type { ReactElement } from 'react';
import { Icon, Kbd, StatusPill } from '../ds';
import { useT } from '../shared/i18n';

interface TitleBarProps {
  page: string;
  onOpenPalette: () => void;
}

export function TitleBar(p: TitleBarProps): ReactElement {
  const t = useT();
  return (
    <header className="hm-titlebar" data-tauri-drag-region>
      <div className="hm-titlebar__lead" data-tauri-drag-region>
        <span className="hm-titlebar__wm" data-tauri-drag-region>{t('app.name')}</span>
        <span className="hm-titlebar__crumb" data-tauri-drag-region>{p.page}</span>
      </div>
      {/* Styled as the SearchField, read-only: it opens the palette. */}
      <button type="button" className="cp-search hm-titlebar__search" aria-haspopup="dialog" onClick={p.onOpenPalette}>
        <Icon name="search" size={18} />
        <span className="hm-titlebar__ph">{t('palette.button')}</span>
        <Kbd keys={['⌘', 'K']} />
      </button>
      <div className="hm-titlebar__trail" data-tauri-drag-region>
        <StatusPill status="local" label={t('common.localOnly')} />
      </div>
    </header>
  );
}
