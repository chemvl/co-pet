// 44 px title bar over the native traffic lights (TitleBarStyle::Overlay): wordmark, page
// name, centred search (⌘K) and the "Local only" status pill. Empty areas drag the window.
import { forwardRef } from 'react';
import { SearchField, StatusPill } from '../ds';
import { useT } from '../shared/i18n';

interface TitleBarProps {
  page: string;
  search: string;
  onSearch: (q: string) => void;
}

export const TitleBar = forwardRef<HTMLInputElement, TitleBarProps>(function TitleBar(p, searchRef) {
  const t = useT();
  return (
    <header className="hm-titlebar" data-tauri-drag-region>
      <div className="hm-titlebar__lead" data-tauri-drag-region>
        <span className="hm-titlebar__wm" data-tauri-drag-region>{t('app.name')}</span>
        <span className="hm-titlebar__crumb" data-tauri-drag-region>{p.page}</span>
      </div>
      <SearchField
        ref={searchRef}
        className="hm-titlebar__search"
        placeholder={t('home.searchPlaceholder')}
        shortcut={['⌘', 'K']}
        value={p.search}
        onChange={(e) => p.onSearch(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            p.onSearch('');
            e.currentTarget.blur();
          }
        }}
      />
      <div className="hm-titlebar__trail" data-tauri-drag-region>
        <StatusPill status="local" label={t('common.localOnly')} />
      </div>
    </header>
  );
});
