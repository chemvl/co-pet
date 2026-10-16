// Integrations › Notion: connect (OAuth or token), pick a database, map its properties.
// `compact` (onboarding Apps step) shows the connect states only.
import { queryOptions, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useId, useRef, useState, type ReactElement } from 'react';
import { Button } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { notionDataSources, notionSchema, setExportTarget } from '../../shared/ipc';
import { INTEGRATIONS_KEY } from '../../shared/queries';
import type { NotionConfig, NotionMapping, NotionSchema, ProviderStatus, TargetOption } from '../../shared/types';
import { propsOf, reconcileMapping, statusMapFor } from './model';
import { ConnectionStates, DisconnectButton, ProviderCard, QueueLine, useIntegrationRun } from './parts';

const schemaQuery = (id: string) =>
  queryOptions({ queryKey: ['notionSchema', id], queryFn: () => notionSchema(id), staleTime: 5 * 60_000 });

export function NotionCard({ status, compact }: { status: ProviderStatus | undefined; compact?: boolean }): ReactElement {
  const t = useT();
  const [changing, setChanging] = useState(false);
  return (
    <ConnectionStates provider="notion" status={status} compact={compact} caption={t('integrations.notion.caption')}>
      {(s, account, connectedPill) => {
        const target = s.target;
        if (!target || changing) {
          return (
            <ProviderCard provider="notion" caption={t('integrations.connectedAs', { account })}>
              <DataSourcePicker
                current={(target?.config as NotionConfig | undefined)?.dataSourceId}
                onPicked={() => setChanging(false)}
              />
              <div className="in-actions">
                {target ? (
                  <Button variant="secondary" size="sm" onClick={() => setChanging(false)}>
                    {t('common.cancel')}
                  </Button>
                ) : null}
                <DisconnectButton provider="notion" />
              </div>
              <QueueLine status={s} />
            </ProviderCard>
          );
        }
        return (
          <ProviderCard provider="notion" caption={`${account} · ${target.targetLabel}`} pill={connectedPill}>
            <MappingGrid config={target.config as NotionConfig} targetLabel={target.targetLabel} />
            <div className="in-actions">
              <Button variant="secondary" size="sm" onClick={() => setChanging(true)}>
                {t('integrations.notion.change')}
              </Button>
              <DisconnectButton provider="notion" />
            </div>
            <QueueLine status={s} />
          </ProviderCard>
        );
      }}
    </ConnectionStates>
  );
}

/** The data sources shared with the integration; picking one saves it with the suggested mapping. */
function DataSourcePicker({ current, onPicked }: { current?: string; onPicked: () => void }): ReactElement {
  const t = useT();
  const qc = useQueryClient();
  const run = useIntegrationRun();
  const headingId = useId();
  const sources = useQuery({ queryKey: ['notionDataSources'], queryFn: notionDataSources });
  const [saving, setSaving] = useState<string | null>(null);

  const pick = async (option: TargetOption) => {
    setSaving(option.id);
    const ok = await run(async () => {
      const schema = await qc.fetchQuery(schemaQuery(option.id));
      await setExportTarget('notion', { dataSourceId: option.id, mapping: schema.suggested }, option.label);
    });
    setSaving(null);
    if (ok) onPicked();
  };

  let body: ReactElement;
  if (sources.isError) {
    body = (
      <div className="in-actions">
        <span className="st-error">{errorMessage(t, sources.error)}</span>
        <Button variant="secondary" size="sm" onClick={() => void sources.refetch()}>
          {t('common.retry')}
        </Button>
      </div>
    );
  } else if (!sources.data) {
    body = <p className="caption in-muted in-m0">{t('integrations.loading')}</p>;
  } else if (!sources.data.length) {
    body = <p className="caption in-muted in-m0">{t('integrations.notion.none')}</p>;
  } else {
    body = (
      <ul className="in-sources" aria-labelledby={headingId}>
        {sources.data.map((s) => (
          <li key={s.id}>
            <button
              type="button"
              className="in-source"
              aria-current={s.id === current || undefined}
              disabled={saving != null}
              onClick={() => void pick(s)}
            >
              <span className="in-source__label">{s.label}</span>
              {saving === s.id ? <span className="caption in-muted">{t('integrations.loading')}</span> : null}
            </button>
          </li>
        ))}
      </ul>
    );
  }
  return (
    <div className="in-picker">
      <span className="label" id={headingId}>{t('integrations.notion.pick')}</span>
      {body}
    </div>
  );
}

/** Co-pet task field → Notion property; every change saves at once through `set_export_target`. */
function MappingGrid({ config, targetLabel }: { config: NotionConfig; targetLabel: string }): ReactElement {
  const t = useT();
  const qc = useQueryClient();
  const run = useIntegrationRun();
  const id = useId();
  const schema = useQuery(schemaQuery(config.dataSourceId));
  const s: NotionSchema | undefined = schema.data;
  const mapping = s ? reconcileMapping(s, config.mapping) : config.mapping;

  // A property renamed or deleted in Notion since the mapping was saved: store the reconciled
  // mapping once, so exports stop sending the vanished property.
  const healed = useRef<string | null>(null);
  useEffect(() => {
    if (!s || healed.current === config.dataSourceId) return;
    healed.current = config.dataSourceId;
    const fixed = reconcileMapping(s, config.mapping);
    if (JSON.stringify(fixed) !== JSON.stringify(config.mapping)) {
      void run(() => setExportTarget('notion', { dataSourceId: config.dataSourceId, mapping: fixed }, targetLabel));
    }
  }, [s, config, targetLabel, run]);

  const save = (patch: Partial<NotionMapping>) => {
    const next: NotionConfig = { dataSourceId: config.dataSourceId, mapping: { ...mapping, ...patch } };
    // Show the choice at once; the refetch after saving confirms it.
    qc.setQueryData<ProviderStatus[]>(INTEGRATIONS_KEY, (list) =>
      list?.map((p) => (p.provider === 'notion' && p.target ? { ...p, target: { ...p.target, config: next } } : p)),
    );
    void run(() => setExportTarget('notion', next, targetLabel));
  };

  /** A `<select>` of the schema's properties of `kinds`; before the schema loads, only `value`. */
  const propSelect = (field: string, value: string | null, kinds: readonly string[], onChange: (v: string | null) => void, nullable = true) => {
    const names = s ? propsOf(s, kinds).map((p) => p.name) : value ? [value] : [];
    return (
      <select
        id={`${id}-${field}`}
        className="st-input in-select"
        value={value ?? ''}
        disabled={!s}
        onChange={(e) => onChange(e.currentTarget.value || null)}
      >
        {nullable ? <option value="">{t('integrations.notMapped')}</option> : null}
        {names.map((n) => (
          <option key={n} value={n}>{n}</option>
        ))}
      </select>
    );
  };

  const statusProp = mapping.status ? s?.props.find((p) => p.name === mapping.status?.prop) : undefined;
  const statusOptions = statusProp?.options ?? (mapping.status ? [mapping.status.todo, mapping.status.done] : []);
  const optionSelect = (which: 'todo' | 'done') => (
    <select
      className="st-input in-select"
      aria-label={t(which === 'todo' ? 'integrations.map.todo' : 'integrations.map.done')}
      value={mapping.status?.[which] ?? ''}
      disabled={!s}
      onChange={(e) => mapping.status && save({ status: { ...mapping.status, [which]: e.currentTarget.value } })}
    >
      {[...new Set(statusOptions)].map((o) => (
        <option key={o} value={o}>{o}</option>
      ))}
    </select>
  );

  const row = (field: string, label: string, control: ReactElement) => (
    <>
      <label className="in-map__field" htmlFor={`${id}-${field}`}>{label}</label>
      <span className="in-map__arrow" aria-hidden="true">→</span>
      {control}
    </>
  );

  return (
    <div className="in-mapping">
      <div className="in-map">
        <span className="caption in-muted">{t('integrations.map.task')}</span>
        <span />
        <span className="caption in-muted">{t('integrations.map.notion')}</span>
        {row('title', t('integrations.map.title'), propSelect('title', mapping.title, ['title'], (v) => v && save({ title: v }), false))}
        {row(
          'status',
          t('integrations.map.status'),
          <div className="in-map__status">
            {propSelect('status', mapping.status?.prop ?? null, ['status', 'select'], (v) => s && save({ status: statusMapFor(s, v) }))}
            {mapping.status ? (
              <div className="in-map__options">
                {optionSelect('todo')}
                <span className="in-muted" aria-hidden="true">/</span>
                {optionSelect('done')}
              </div>
            ) : null}
          </div>,
        )}
        {row('due', t('integrations.map.due'), propSelect('due', mapping.due, ['date'], (v) => save({ due: v })))}
        {row('tags', t('integrations.map.tags'), propSelect('tags', mapping.tags, ['multi_select'], (v) => save({ tags: v })))}
        {row('source', t('integrations.map.source'), propSelect('source', mapping.source, ['url'], (v) => save({ source: v })))}
      </div>
      {schema.isError ? (
        <div className="in-actions">
          <span className="st-error">{errorMessage(t, schema.error)}</span>
          <Button variant="secondary" size="sm" onClick={() => void schema.refetch()}>
            {t('common.retry')}
          </Button>
        </div>
      ) : null}
    </div>
  );
}
