// Integrations › Google Tasks: connect (OAuth only; Google has no personal tokens), then pick a
// task list. `compact` (onboarding Apps step) shows the connect states only.
import { useQuery } from '@tanstack/react-query';
import { useId, useState, type ReactElement } from 'react';
import { Button } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { googleTasksLists, setExportTarget } from '../../shared/ipc';
import type { GoogleTasksConfig, ProviderStatus, TargetOption } from '../../shared/types';
import { ConnectionStates, DisconnectButton, ProviderCard, QueueLine, useIntegrationRun } from './parts';

export function GoogleTasksCard({ status, compact }: { status: ProviderStatus | undefined; compact?: boolean }): ReactElement {
  const t = useT();
  const [changing, setChanging] = useState(false);
  return (
    <ConnectionStates provider="google_tasks" status={status} compact={compact} caption={t('integrations.googleTasks.caption')}>
      {(s, account, connectedPill) => {
        const target = s.target;
        if (!target || changing) {
          return (
            <ProviderCard provider="google_tasks" caption={t('integrations.connectedAs', { account })}>
              <ListPicker
                current={(target?.config as GoogleTasksConfig | undefined)?.tasklistId}
                onPicked={() => setChanging(false)}
              />
              <div className="in-actions">
                {target ? (
                  <Button variant="secondary" size="sm" onClick={() => setChanging(false)}>
                    {t('common.cancel')}
                  </Button>
                ) : null}
                <DisconnectButton provider="google_tasks" />
              </div>
              <QueueLine status={s} />
            </ProviderCard>
          );
        }
        return (
          <ProviderCard provider="google_tasks" caption={`${account} · ${target.targetLabel}`} pill={connectedPill}>
            <p className="caption in-muted in-m0">{t('integrations.googleTasks.note')}</p>
            <div className="in-actions">
              <Button variant="secondary" size="sm" onClick={() => setChanging(true)}>
                {t('integrations.googleTasks.change')}
              </Button>
              <DisconnectButton provider="google_tasks" />
            </div>
            <QueueLine status={s} />
          </ProviderCard>
        );
      }}
    </ConnectionStates>
  );
}

/** The account's task lists; picking one saves it as the target. */
function ListPicker({ current, onPicked }: { current?: string; onPicked: () => void }): ReactElement {
  const t = useT();
  const run = useIntegrationRun();
  const headingId = useId();
  const lists = useQuery({ queryKey: ['googleTasksLists'], queryFn: googleTasksLists });
  const [saving, setSaving] = useState<string | null>(null);

  const pick = async (option: TargetOption) => {
    setSaving(option.id);
    const ok = await run(() => setExportTarget('google_tasks', { tasklistId: option.id }, option.label));
    setSaving(null);
    if (ok) onPicked();
  };

  let body: ReactElement;
  if (lists.isError) {
    body = (
      <div className="in-actions">
        <span className="st-error">{errorMessage(t, lists.error)}</span>
        <Button variant="secondary" size="sm" onClick={() => void lists.refetch()}>
          {t('common.retry')}
        </Button>
      </div>
    );
  } else if (!lists.data) {
    body = <p className="caption in-muted in-m0">{t('integrations.loading')}</p>;
  } else if (!lists.data.length) {
    body = <p className="caption in-muted in-m0">{t('integrations.googleTasks.none')}</p>;
  } else {
    body = (
      <ul className="in-sources" aria-labelledby={headingId}>
        {lists.data.map((l) => (
          <li key={l.id}>
            <button
              type="button"
              className="in-source"
              aria-current={l.id === current || undefined}
              disabled={saving != null}
              onClick={() => void pick(l)}
            >
              <span className="in-source__label">{l.label}</span>
              {saving === l.id ? <span className="caption in-muted">{t('integrations.loading')}</span> : null}
            </button>
          </li>
        ))}
      </ul>
    );
  }
  return (
    <div className="in-picker">
      <span className="label" id={headingId}>{t('integrations.googleTasks.pick')}</span>
      {body}
    </div>
  );
}
