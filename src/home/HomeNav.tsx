// Home sidebar: pet avatar + model status, Inbox, Tasks, the live categories with counts
// (Unsorted's is a red badge and stays last), Settings at the bottom.
import { useQuery } from '@tanstack/react-query';
import type { ReactElement, ReactNode } from 'react';
import { Icon, type IconName } from '../ds';
import { CatArt, LOOKS, type CatSkin } from '../pet/CatArt';
import { poseFor } from '../pet/pose';
import { categoryColor, categoryIcon, categoryLabel } from '../shared/categories';
import { useT, type TFunction } from '../shared/i18n';
import { useAiStatus, useCategories, useSettings } from '../shared/queries';
import type { AiStatus } from '../shared/types';
import { countsQuery } from './queries';

/** `inbox`, `tasks`, `settings` or a category id; empty for none. */
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
  const { data: categories } = useCategories();
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
      <NavLink
        href="#/tasks"
        active={active === 'tasks'}
        icon="tasks"
        label={t('tasks.title')}
        count={<span className="hm-nav__count">{c?.tasksOpen ?? ''}</span>}
      />
      <div className="hm-nav__heading">{t('home.categories')}</div>
      {categories?.map((cat) => (
        <NavLink
          key={cat.id}
          href={cat.id === 'unsorted' ? '#/inbox/unsorted' : `#/category/${cat.id}`}
          active={active === cat.id}
          icon={categoryIcon(cat)}
          iconClass={`hm-c-${categoryColor(cat)}`}
          label={categoryLabel(cat, t)}
          count={
            cat.id === 'unsorted' ? (
              unsorted > 0 ? (
                <span className="hm-nav__count is-attn" aria-label={t('pet.badge.unsorted', { n: unsorted })}>
                  {unsorted > 99 ? '99+' : unsorted}
                </span>
              ) : null
            ) : (
              <span className="hm-nav__count">{c?.byCategory[cat.id] ?? ''}</span>
            )
          }
        />
      ))}
      <div className="hm-nav__spacer" />
      <NavLink href="#/settings/general" active={active === 'settings'} icon="settings" label={t('common.settings')} />
    </nav>
  );
}
