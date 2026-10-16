// Integrations › Asana: connect (OAuth or token), then Workspace → Project → Section. Tasks are
// assigned to the connected user. `compact` (onboarding Apps step) shows the connect states only.
import { useQuery } from '@tanstack/react-query';
import { useId, useState, type ReactElement } from 'react';
import { Button, StatusPill } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { asanaOptions, setExportTarget } from '../../shared/ipc';
import type { AsanaConfig, ProviderStatus } from '../../shared/types';
import { ConnectSection, DisconnectButton, ProviderCard, QueueLine, useIntegrationRun } from './parts';

export function AsanaCard({ status, compact }: { status: ProviderStatus | undefined; compact?: boolean }): ReactElement {
  const t = useT();
  const [changing, setChanging] = useState(false);

  if (!status?.connected) {
    return (
      <ProviderCard provider="asana" caption={t('integrations.asana.caption')}>
        <ConnectSection provider="asana" compact={compact} />
      </ProviderCard>
    );
  }
  const account = status.accountLabel ?? '';
  if (status.needsReconnect) {
    return (
      <ProviderCard
        provider="asana"
        caption={t('integrations.connectedAs', { account })}
        pill={<StatusPill status="failed" label={t('integrations.reconnectNeeded')} />}
      >
        <ConnectSection provider="asana" reconnect compact={compact} />
        {compact ? null : <QueueLine status={status} />}
      </ProviderCard>
    );
  }
  const connectedPill = <StatusPill status="synced" label={t('integrations.connected')} />;
  const target = status.target;
  if (compact) {
    return <ProviderCard provider="asana" caption={t('integrations.connectedAs', { account })} pill={connectedPill} />;
  }
  if (!target || changing) {
    return (
      <ProviderCard provider="asana" caption={t('integrations.connectedAs', { account })}>
        <TargetPicker current={target?.config as AsanaConfig | undefined} onSave={() => setChanging(true)} />
        <div className="in-actions">
          {target ? (
            <Button variant="secondary" size="sm" onClick={() => setChanging(false)}>
              {t('common.done')}
            </Button>
          ) : null}
          <DisconnectButton provider="asana" />
        </div>
        <QueueLine status={status} />
      </ProviderCard>
    );
  }
  return (
    <ProviderCard
      provider="asana"
      caption={t('integrations.asana.target', { target: target.targetLabel })}
      pill={connectedPill}
    >
      <p className="caption in-muted in-m0">{t('integrations.asana.note')}</p>
      <div className="in-actions">
        <Button variant="secondary" size="sm" onClick={() => setChanging(true)}>
          {t('integrations.asana.change')}
        </Button>
        <DisconnectButton provider="asana" />
      </div>
      <QueueLine status={status} />
    </ProviderCard>
  );
}

/**
 * Workspace → Project → Section. Picking a project (then a section) saves the target at once;
 * `onSave` keeps the picker open so a section can still be chosen.
 */
function TargetPicker({ current, onSave }: { current?: AsanaConfig; onSave: () => void }): ReactElement {
  const t = useT();
  const run = useIntegrationRun();
  const id = useId();
  const [workspace, setWorkspace] = useState(current?.workspaceGid ?? '');
  const [project, setProject] = useState(current?.projectGid ?? '');
  const [section, setSection] = useState(current?.sectionGid ?? '');

  const workspaces = useQuery({ queryKey: ['asana', 'workspace'], queryFn: () => asanaOptions('workspace') });
  const projects = useQuery({
    queryKey: ['asana', 'project', workspace],
    queryFn: () => asanaOptions('project', workspace),
    enabled: !!workspace,
  });
  const sections = useQuery({
    queryKey: ['asana', 'section', project],
    queryFn: () => asanaOptions('section', project),
    enabled: !!project,
  });
  const error = workspaces.error ?? projects.error ?? sections.error;

  const save = (projectGid: string, sectionGid: string) => {
    onSave();
    const projectLabel = projects.data?.find((o) => o.id === projectGid)?.label ?? projectGid;
    const sectionLabel = sections.data?.find((o) => o.id === sectionGid)?.label;
    const label = sectionGid && sectionLabel ? `${projectLabel} › ${sectionLabel}` : projectLabel;
    void run(() =>
      setExportTarget('asana', { workspaceGid: workspace, projectGid, sectionGid: sectionGid || null }, label),
    );
  };

  const field = (
    key: 'workspace' | 'project' | 'section',
    value: string,
    options: { id: string; label: string }[] | undefined,
    onChange: (v: string) => void,
    disabled: boolean,
  ) => (
    <div className="in-field">
      <label className="label" htmlFor={`${id}-${key}`}>{t(`integrations.asana.${key}`)}</label>
      <select
        id={`${id}-${key}`}
        className="st-input in-select"
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.currentTarget.value)}
      >
        <option value="" disabled={key !== 'section'}>
          {key === 'section' ? t('integrations.asana.noSection') : t('integrations.asana.choose')}
        </option>
        {options?.map((o) => (
          <option key={o.id} value={o.id}>{o.label}</option>
        ))}
      </select>
    </div>
  );

  return (
    <div className="in-picker">
      {field('workspace', workspace, workspaces.data, (v) => {
        setWorkspace(v);
        setProject('');
        setSection('');
      }, !workspaces.data)}
      {field('project', project, projects.data, (v) => {
        setProject(v);
        setSection('');
        save(v, '');
      }, !workspace || !projects.data)}
      {field('section', section, sections.data, (v) => {
        setSection(v);
        save(project, v);
      }, !project || !sections.data)}
      {error ? <p className="st-error" role="alert">{errorMessage(t, error)}</p> : null}
    </div>
  );
}
