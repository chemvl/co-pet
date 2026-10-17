// Home › Agents (screen 12, Agents.dc.html): the MCP server state, one card per client (Add,
// Approve writes, Manage), the stdio command and local HTTP address, and the activity log
// with Undo. Every query lives under `AGENTS_KEY`, refreshed by `agents-changed`.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useId, useState, type ReactElement, type ReactNode } from 'react';
import { Button, Icon, SegmentedControl, StatusPill } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { formatDate, formatTime } from '../../shared/format';
import { useLang, useT } from '../../shared/i18n';
import {
  addMcpClient, approveMcpClient, createHttpClient, listAgentActivity, mcpStatus, removeMcpClient, revokeMcpClient,
  setMcpPaused, undoAgentActivity,
} from '../../shared/ipc';
import { AGENTS_KEY, useCategories, useMcpClients } from '../../shared/queries';
import { useToast } from '../../shared/Toast';
import type { ActivityRow, KnownMcpClient, McpClientView } from '../../shared/types';
import { PROVIDER_NAME } from '../integrations/model';
import { MenuButton } from '../Menu';
import { QueryState } from '../InboxPage';
import { useNow } from '../hooks';
import { activityAction, activityText, relativeTime } from './activityText';
import { Dialog, useCopied } from '../settings/parts';
import '../settings/settings.css';
import './agents.css';

const KNOWN: readonly KnownMcpClient[] = ['claude-code', 'cursor', 'codex', 'gemini-cli'];

function isKnown(key: string): key is KnownMcpClient {
  return (KNOWN as readonly string[]).includes(key);
}

const ACTIVITY_LIMIT = 100;

type Show = 'all' | 'writes';

/** Runs an action and marks it busy; a failure toasts and re-reads the Agents queries (a success
 *  refreshes them through `agents-changed`). */
function useAgentsRun(): { busy: string | null; run: <T>(key: string, op: () => Promise<T>) => Promise<T | undefined> } {
  const t = useT();
  const qc = useQueryClient();
  const { show } = useToast();
  const [busy, setBusy] = useState<string | null>(null);
  const run = async <T,>(key: string, op: () => Promise<T>): Promise<T | undefined> => {
    setBusy(key);
    try {
      return await op();
    } catch (e) {
      show({ message: errorMessage(t, e) });
      void qc.invalidateQueries({ queryKey: AGENTS_KEY });
      return undefined;
    } finally {
      setBusy(null);
    }
  };
  return { busy, run };
}

export function AgentsPage(): ReactElement {
  const t = useT();
  const otherId = useId();
  const clientsQuery = useMcpClients();
  const statusQuery = useQuery({ queryKey: [...AGENTS_KEY, 'status'], queryFn: mcpStatus, staleTime: Infinity });
  const clients = clientsQuery.data;
  const status = statusQuery.data;
  const { busy, run } = useAgentsRun();
  const now = useNow();
  const [token, setToken] = useState<{ name: string; token: string } | null>(null);
  const [removing, setRemoving] = useState<McpClientView | null>(null);
  const [naming, setNaming] = useState(false);
  const paused = status?.paused ?? false;

  const approve = async (c: McpClientView, id: string) => {
    const got = await run(`approve:${id}`, () => approveMcpClient(id));
    if (got) setToken({ name: c.name, token: got.token });
  };

  return (
    <div className="hm-page">
      <div className="ag-page">
        <div className="ag-head">
          <h1 className="title ag-m0">{t('agents.title')}</h1>
          {status ? (
            <StatusPill status={paused ? 'pending' : 'synced'} label={t(paused ? 'agents.paused' : 'agents.running')} />
          ) : null}
          <div className="ag-grow" />
          <Button
            variant="secondary"
            size="sm"
            icon={paused ? 'play' : 'pause'}
            disabled={!status || busy === 'pause'}
            onClick={() => void run('pause', () => setMcpPaused(!paused))}
          >
            {t(paused ? 'agents.resume' : 'agents.pause')}
          </Button>
        </div>
        {/* The status loads with the clients; only its failure needs its own line. */}
        {statusQuery.isError ? <QueryState q={statusQuery} loading={t('agents.loading')} /> : null}
        <QueryState q={clientsQuery} loading={t('agents.loading')} />
        <section className="ag-grid" aria-label={t('agents.clients')}>
          {clients?.map((c) => (
            <ClientCard
              key={c.id ?? c.key}
              client={c}
              now={now}
              busy={busy}
              onAdd={(key) => void run(`add:${key}`, () => addMcpClient(key))}
              onApprove={(id) => void approve(c, id)}
              onRevoke={(id) => void run(`revoke:${id}`, () => revokeMcpClient(id))}
              onRemove={() => setRemoving(c)}
            />
          ))}
        </section>

        <section aria-labelledby={otherId}>
          <p className="ag-h" id={otherId}>{t('agents.other')}</p>
          {status ? (
            <div className="ag-codes">
              <CodeBox text={status.stdioCommand} copyLabel={t('agents.copyCommand')} />
              <CodeBox
                text={status.httpUrl}
                copyLabel={t('agents.copyUrl')}
                pill={
                  status.httpState === 'portInUse' ? (
                    <StatusPill status="failed" label={t('agents.portBusy', { port: new URL(status.httpUrl).port })} />
                  ) : null
                }
              />
            </div>
          ) : null}
          <div className="caption ag-muted ag-cap">{t('agents.otherCaption')}</div>
          <div className="ag-other__actions">
            <Button variant="quiet" size="sm" icon="plus" onClick={() => setNaming(true)}>
              {t('agents.createToken')}
            </Button>
          </div>
        </section>

        <Activity />
      </div>

      {token ? <TokenDialog name={token.name} token={token.token} onClose={() => setToken(null)} /> : null}
      {removing ? (
        <RemoveDialog
          client={removing}
          busy={busy === `remove:${removing.id}`}
          onClose={() => setRemoving(null)}
          onConfirm={async () => {
            const id = removing.id;
            if (id) await run(`remove:${id}`, () => removeMcpClient(id));
            setRemoving(null);
          }}
        />
      ) : null}
      {naming ? (
        <NameDialog
          onClose={() => setNaming(false)}
          onCreated={(name, value) => {
            setNaming(false);
            setToken({ name, token: value });
          }}
        />
      ) : null}
    </div>
  );
}

function ClientCard(p: {
  client: McpClientView;
  now: number;
  busy: string | null;
  onAdd: (key: KnownMcpClient) => void;
  onApprove: (id: string) => void;
  onRevoke: (id: string) => void;
  onRemove: () => void;
}): ReactElement {
  const t = useT();
  const lang = useLang();
  const c = p.client;
  const { id, key } = c;
  let body: ReactNode;
  if (id && c.writeApproved) {
    const date = c.approvedAt != null ? formatDate(c.approvedAt, lang) : '';
    body = (
      <>
        <StatusPill status="synced" label={t('agents.readWrite')} />
        <span className="caption ag-muted">
          {c.lastCallAt != null
            ? t('agents.approvedLastCall', { date, when: relativeTime(c.lastCallAt, p.now, t, lang) })
            : t('agents.approved', { date })}
        </span>
        <MenuButton
          variant="quiet"
          size="sm"
          menuLabel={t('agents.manageMenu', { name: c.name })}
          items={[
            { key: 'revoke', label: t('agents.revoke'), onSelect: () => p.onRevoke(id) },
            { key: 'remove', label: t('agents.remove', { name: c.name }), danger: true, onSelect: p.onRemove },
          ]}
        >
          {t('agents.manage')}
        </MenuButton>
      </>
    );
  } else if (id) {
    body = (
      <>
        <StatusPill status="pending" label={t('agents.readOnly')} />
        <span className="caption ag-muted">
          {t('agents.connected', { date: c.createdAt != null ? formatDate(c.createdAt, lang) : '' })}
        </span>
        <Button variant="secondary" size="sm" disabled={p.busy === `approve:${id}`} onClick={() => p.onApprove(id)}>
          {t('agents.approve')}
        </Button>
      </>
    );
  } else {
    body = (
      <>
        <span className="caption ag-muted">{t('agents.notAdded')}</span>
        {isKnown(key) ? (
          <Button variant="secondary" size="sm" icon="plus" disabled={p.busy === `add:${key}`} onClick={() => p.onAdd(key)}>
            {t('agents.add', { name: c.name })}
          </Button>
        ) : null}
      </>
    );
  }
  return (
    <article className="ag-card" aria-label={c.name}>
      <div className="ag-card__name">
        <Icon name="agent" size={20} />
        <b title={c.name}>{c.name}</b>
      </div>
      {body}
    </article>
  );
}

function CodeBox(p: { text: string; copyLabel: string; pill?: ReactNode }): ReactElement {
  const [copied, copy] = useCopied();
  return (
    <div className="ag-code">
      <span className="ag-code__text" title={p.text}>{p.text}</span>
      {p.pill}
      <Button
        variant="quiet"
        size="sm"
        icon={copied ? 'check' : 'copy'}
        aria-label={p.copyLabel}
        onClick={() => copy(p.text)}
      />
    </div>
  );
}

function Activity(): ReactElement {
  const t = useT();
  const headId = useId();
  const [show, setShow] = useState<Show>('all');
  const activity = useQuery({
    queryKey: [...AGENTS_KEY, 'activity', show],
    queryFn: () => listAgentActivity(show === 'writes', ACTIVITY_LIMIT),
    staleTime: Infinity,
  });
  const rows = activity.data;
  const { data: categories } = useCategories();
  const { busy, run } = useAgentsRun();
  const { show: toast } = useToast();

  const action = (row: ActivityRow): ReactNode => {
    const a = activityAction(row);
    switch (a?.kind) {
      case 'undo':
      case 'undone':
        return (
          <Button
            variant="quiet"
            size="sm"
            icon="undo"
            disabled={a.kind === 'undone' || busy === row.id}
            onClick={() => void run(row.id, () => undoAgentActivity(row.id))}
          >
            {t(a.kind === 'undone' ? 'agents.undone' : 'common.undo')}
          </Button>
        );
      case 'read':
        return <span className="caption ag-muted">{t('agents.read')}</span>;
      case 'open':
        return (
          <button
            type="button"
            className="hm-link"
            onClick={() => void openUrl(a.url).catch((e) => toast({ message: errorMessage(t, e) }))}
          >
            {t('tasks.openIn', { provider: PROVIDER_NAME[a.provider] })}
          </button>
        );
      default:
        return null;
    }
  };

  return (
    <section aria-labelledby={headId}>
      <div className="ag-activity__head">
        <p className="ag-h" id={headId}>{t('agents.activity')}</p>
        <SegmentedControl
          label={t('agents.show')}
          value={show}
          onChange={(v) => setShow(v === 'writes' ? 'writes' : 'all')}
          options={[
            { value: 'all', label: t('common.all') },
            { value: 'writes', label: t('agents.changesOnly') },
          ]}
        />
      </div>
      <table className="ag-table">
        <thead>
          <tr>
            <th className="ag-c-time">{t('agents.col.time')}</th>
            <th className="ag-c-agent">{t('agents.col.agent')}</th>
            <th className="ag-c-tool">{t('agents.col.tool')}</th>
            <th>{t('agents.col.what')}</th>
            <th className="ag-c-action"><span className="ag-sr">{t('agents.col.action')}</span></th>
          </tr>
        </thead>
        <tbody>
          {rows?.map((row) => {
            const what = activityText(row, t, categories);
            return (
              <tr key={row.id}>
                <td>{formatTime(row.at)}</td>
                <td title={row.clientName}>{row.clientName}</td>
                <td className="ag-mono" title={row.tool}>{row.tool}</td>
                <td title={what}>{what}</td>
                <td>{action(row)}</td>
              </tr>
            );
          })}
          {rows && rows.length === 0 ? (
            <tr>
              <td colSpan={5} className="caption ag-muted ag-empty">{t('agents.empty')}</td>
            </tr>
          ) : null}
        </tbody>
      </table>
      <QueryState q={activity} loading={t('agents.loadingActivity')} />
      <div className="caption ag-muted ag-foot">{t('agents.footer')}</div>
    </section>
  );
}

/** The new HTTP token, shown once. */
function TokenDialog(p: { name: string; token: string; onClose: () => void }): ReactElement {
  const t = useT();
  const [copied, copy] = useCopied();
  return (
    <Dialog
      title={t('agents.tokenTitle', { name: p.name })}
      onClose={p.onClose}
      actions={
        <Button variant="primary" autoFocus onClick={p.onClose}>
          {t('common.done')}
        </Button>
      }
    >
      <div className="ag-token">
        <span className="ag-token__text">{p.token}</span>
        <Button variant="secondary" size="sm" icon={copied ? 'check' : 'copy'} aria-label={t('agents.copyToken')} onClick={() => copy(p.token)}>
          {t(copied ? 'common.copied' : 'common.copy')}
        </Button>
      </div>
      <p className="body ag-m0">{t('agents.tokenNote')}</p>
    </Dialog>
  );
}

function RemoveDialog(p: {
  client: McpClientView;
  busy: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}): ReactElement {
  const t = useT();
  const name = p.client.name;
  return (
    <Dialog
      title={t('agents.removeTitle', { name })}
      role="alertdialog"
      onClose={p.onClose}
      actions={
        <>
          <Button variant="quiet" autoFocus onClick={p.onClose}>{t('common.cancel')}</Button>
          <Button variant="danger" disabled={p.busy} onClick={() => void p.onConfirm()}>
            {t('agents.removeConfirm')}
          </Button>
        </>
      }
    >
      <p className="body ag-m0">{t('agents.removeBody', { name })}</p>
    </Dialog>
  );
}

/** "Create HTTP token": a name, then the token dialog. */
function NameDialog(p: { onClose: () => void; onCreated: (name: string, token: string) => void }): ReactElement {
  const t = useT();
  const inputId = useId();
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const create = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const created = await createHttpClient(name.trim());
      p.onCreated(created.client.name, created.token);
    } catch (e) {
      // Nothing was created; a success refreshes the cards through `agents-changed`.
      setError(errorMessage(t, e));
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={t('agents.createToken')}
      onClose={p.onClose}
      actions={
        <>
          <Button variant="quiet" onClick={p.onClose}>{t('common.cancel')}</Button>
          <Button variant="primary" disabled={!name.trim() || busy} onClick={() => void create()}>
            {t('agents.tokenCreate')}
          </Button>
        </>
      }
    >
      <label className="st-row__label" htmlFor={inputId}>{t('agents.tokenName')}</label>
      <input
        id={inputId}
        className="st-input"
        value={name}
        autoFocus
        autoComplete="off"
        spellCheck={false}
        maxLength={100}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') void create();
        }}
      />
      {error ? <p className="st-error" role="alert">{error}</p> : null}
    </Dialog>
  );
}
