// Home › Integrations (screen 11): export targets on the left, AI models on the right.
import { useId, type ReactElement } from 'react';
import { Button, Icon } from '../../ds';
import { useT } from '../../shared/i18n';
import { useIntegrations } from '../../shared/queries';
import { ModelCards } from '../settings/ModelCards';
import { AsanaCard } from './AsanaCard';
import { NotionCard } from './NotionCard';
import './integrations.css';

/** Exporters planned for later releases (product names, not translated). */
const LATER = ['Linear', 'Todoist', 'Jira', 'GitHub Issues', 'Google Tasks', 'Markdown / CSV'] as const;

export function IntegrationsPage(): ReactElement {
  const t = useT();
  const exportId = useId();
  const aiId = useId();
  const { data: statuses } = useIntegrations();
  const notion = statuses?.find((s) => s.provider === 'notion');
  const asana = statuses?.find((s) => s.provider === 'asana');

  return (
    <div className="hm-page">
      <div className="in-page">
        <h1 className="title in-m0">{t('integrations.title')}</h1>
        <div className="in-cols">
          <section className="in-col" aria-labelledby={exportId}>
            <h2 className="in-h" id={exportId}>{t('integrations.exportTasks')}</h2>
            <NotionCard status={notion} />
            <AsanaCard status={asana} />
            <div className="in-later">
              <span className="caption in-muted">{t('integrations.later')}</span>
              {LATER.map((name) => (
                <span key={name} className="in-later__chip">{name}</span>
              ))}
            </div>
            <p className="caption in-muted in-m0">{t('integrations.oneWay')}</p>
          </section>
          <section className="in-col" aria-labelledby={aiId}>
            <h2 className="in-h" id={aiId}>{t('integrations.aiModels')}</h2>
            <article className="in-card" aria-label={t('settings.triage.model')}>
              <div className="in-row">
                <Icon name="idea" size={20} />
                <b className="in-row__name">{t('settings.triage.model')}</b>
              </div>
              <ModelCards />
            </article>
            <article className="in-card is-disabled" aria-disabled="true" aria-label={t('integrations.cloud.title')}>
              <div className="in-row">
                <Icon name="lock" size={20} />
                <div className="in-row__text">
                  <b className="in-row__name">{t('integrations.cloud.title')}</b>
                  {t('integrations.cloud.desc')}
                </div>
                <Button variant="secondary" size="sm" disabled>
                  {t('integrations.cloud.addKey')}
                </Button>
              </div>
              <span className="caption in-muted">{t('integrations.cloud.later')}</span>
            </article>
          </section>
        </div>
      </div>
    </div>
  );
}
