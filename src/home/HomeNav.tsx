// Home sidebar: pet avatar + model status, Inbox, the nine categories with counts (Unsorted's
// is a red badge), Settings at the bottom.
import { useQuery } from '@tanstack/react-query';
import type { ReactElement, ReactNode } from 'react';
import { Icon, type IconName } from '../ds';
import { CatArt, LOOKS, type CatSkin } from '../pet/CatArt';
import { poseFor } from '../pet/pose';
import { useT, type TFunction } from '../shared/i18n';
import { useAiStatus, useSettings } from '../shared/queries';
import { CATEGORY_IDS, type AiStatus } from '../shared/types';
import { categoryKey } from './itemText';
import { countsQuery } from './queries';

/** `inbox`, `settings` or a category id; empty for none. */
export type NavKey = string;

const SKINS: readonly CatSkin[] = ['peach', 'tabby', 'midnight'];
const IDLE_PARTS = poseFor('idle').parts;

function modelStatus(ai: AiStatus | undefined, t: TFunction): string {
  if (ai?.state === 'ready') return t('ai.ready');
  if ((ai?.state === 'downloading' || ai?.state === 'verifying') && ai.bytesTotal > 0) {
    return t('ai.downloading', { percent: Math.floor((ai.bytesDone / ai.bytesTotal) * 100) });
  }
  return t('ai.rulesOnly');
}

function NavLink(p: { href: string; active: boolean; icon: IconName; iconClass?: string; label: string; count?: ReactNode }): ReactElement {
  return (
    <a href={p.href} className={`hm-nav__item${p.active ? ' is-active' : ''}`} aria-current={p.active ? 'page' : undefined}>
      <span className={`hm-nav__icon ${p.iconClass ?? ''}`}><Icon name={p.icon} size={18} /></span>
      <span className="hm-nav__label">{p.label}</span>
      {p.count}
    </a>
  );
}

export function HomeNav({ active }: { active: NavKey }): ReactElement {
  const t = useT();
  const { data: c } = useQuery(countsQuery);
  const { data: settings } = useSettings();
  const { data: ai } = useAiStatus();
  const petName = typeof settings?.['pet.name'] === 'string' && settings['pet.name'] ? settings['pet.name'] : 'Miso';
  const look = LOOKS.find((l) => l === settings?.['pet.style']) ?? 'soft';
  const skin = SKINS.find((s) => s === settings?.['pet.skin']) ?? 'peach';
  const unsorted = c?.unsorted ?? 0;

  return (
    <nav className="hm-nav" aria-label={t('home.nav')}>
      <div className="hm-nav__pet">
        <div className="hm-nav__avatar">
          <CatArt look={look} skin={skin} parts={IDLE_PARTS} motion={null} mirrored={false} reducedMotion />
        </div>
        <div className="hm-nav__petText">
          <div className="pet-name hm-nav__petName">{petName}</div>
          <div className="caption hm-muted">{modelStatus(ai, t)}</div>
        </div>
      </div>
      <NavLink
        href="#/inbox"
        active={active === 'inbox'}
        icon="inbox"
        label={t('home.inbox')}
        count={<span className="hm-nav__count">{c?.inbox ?? ''}</span>}
      />
      <div className="hm-nav__heading">{t('home.categories')}</div>
      {CATEGORY_IDS.map((id) => (
        <NavLink
          key={id}
          href={id === 'unsorted' ? '#/inbox/unsorted' : `#/category/${id}`}
          active={active === id}
          icon={id}
          iconClass={`hm-c-${id}`}
          label={t(categoryKey(id))}
          count={
            id === 'unsorted' ? (
              unsorted > 0 ? (
                <span className="hm-nav__count is-attn" aria-label={t('pet.badge.unsorted', { n: unsorted })}>
                  {unsorted > 99 ? '99+' : unsorted}
                </span>
              ) : null
            ) : (
              <span className="hm-nav__count">{c?.byCategory[id] ?? ''}</span>
            )
          }
        />
      ))}
      <div className="hm-nav__spacer" />
      <NavLink href="#/settings/general" active={active === 'settings'} icon="settings" label={t('common.settings')} />
    </nav>
  );
}
