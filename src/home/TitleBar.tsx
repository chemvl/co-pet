// 44 px title bar: over the native traffic lights on macOS, with caption buttons on the right on
// Windows. Wordmark, page name, the centred search button that opens the command palette (⌘K /
// Ctrl+K) and the sync status pill, which opens Settings › Account. Empty areas drag the window.
import type { ReactElement } from 'react';
import { Icon, Kbd, StatusPill } from '../ds';
import { useT } from '../shared/i18n';
import { isWindows } from '../shared/platform';
import { useSyncStatus } from '../shared/queries';
import { syncPill } from '../shared/sync';
import { modKey } from './settings/accelerator';
import { WindowControls } from './WindowControls';

interface TitleBarProps {
  page: string;
  onOpenPalette: () => void;
  onNavigate: (route: string) => void;
}

export function TitleBar(p: TitleBarProps): ReactElement {
  const t = useT();
  const pill = syncPill(useSyncStatus().data?.state ?? 'off');
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
        <Kbd keys={[modKey(), 'K']} />
      </button>
      <div className="hm-titlebar__trail" data-tauri-drag-region>
        <button type="button" className="hm-titlebar__sync" onClick={() => p.onNavigate('settings/account')}>
          <StatusPill status={pill.status} icon={pill.icon} label={t(pill.label)} />
        </button>
        {isWindows && <WindowControls />}
      </div>
    </header>
  );
}
