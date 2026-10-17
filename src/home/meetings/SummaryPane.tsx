// The Meetings summary pane: Copy as Markdown, the summary language, Summary · Decisions ·
// Action items · Open questions, and the changes the meeting proposes to existing tasks.
import { useQueries } from '@tanstack/react-query';
import { useState, type ReactElement, type ReactNode } from 'react';
import { Button } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { useLang, useT, type I18nKey, type TFunction } from '../../shared/i18n';
import {
  addActionToTasks, applyMeetingChange, copyMeetingMarkdown, ignoreMeetingChange, resummarizeMeeting, summarizeMeeting,
  undoMeetingChange,
} from '../../shared/ipc';
import { useToast } from '../../shared/Toast';
import type { ActionItem, MeetingDetail, TaskChange } from '../../shared/types';
import { MenuButton } from '../Menu';
import { taskQuery } from '../queries';
import { formatDay, speakerName } from './segments';

export const SUMMARY_LANGS = ['en', 'vi', 'zh'] as const;
export type SummaryLang = (typeof SUMMARY_LANGS)[number];

export const LANG_NAME: Record<string, I18nKey> = { en: 'lang.en', vi: 'lang.vi', zh: 'lang.zh' };

/** The language the summary is in now (English when unknown). */
export function summaryLangOf(m: MeetingDetail): SummaryLang {
  return SUMMARY_LANGS.find((l) => l === m.summaryLang) ?? 'en';
}

const SKIPPED: Record<string, I18nKey> = {
  aiOff: 'meetings.summary.aiOff',
  private: 'meetings.summary.private',
  sensitive: 'meetings.summary.sensitive',
  noSpeech: 'meetings.noSpeech',
};

function Section({ title, children }: { title: string; children: ReactNode }): ReactElement {
  return (
    <section>
      <h2 className="mt-k">{title}</h2>
      {children}
    </section>
  );
}

/** The owner's current speaker name when the model tied it to a renamed speaker. */
function ownerOf(a: ActionItem, m: MeetingDetail, t: TFunction): string | null {
  const sp = a.ownerKey ? m.speakers.find((s) => s.key === a.ownerKey) : undefined;
  if (sp?.name) return sp.name;
  return a.owner ?? (a.ownerKey ? speakerName({ key: a.ownerKey, name: null }, t) : null);
}

export function SummaryPane({ meeting: m }: { meeting: MeetingDetail }): ReactElement {
  const t = useT();
  const lang = useLang();
  const { show } = useToast();
  const fail = (e: unknown) => show({ message: errorMessage(t, e) });
  const [busy, setBusy] = useState<string | null>(null);
  // Task titles for the change cards.
  const tasks = useQueries({ queries: m.changes.map((c) => ({ ...taskQuery(c.taskId), retry: false })) });

  /** Runs one request at a time per change/action id; errors toast. */
  const run = (id: string, work: () => Promise<unknown>) => {
    setBusy(id);
    work()
      .catch(fail)
      .finally(() => setBusy(null));
  };

  const summaryDone = m.summaryState === 'done';
  const current = summaryLangOf(m);
  const bar =
    m.state === 'done' ? (
      <div className="mt-sp__bar">
        <Button
          variant="secondary"
          size="sm"
          icon="copy"
          onClick={() => copyMeetingMarkdown(m.id).then(() => show({ message: t('common.copied') }), fail)}
        >
          {t('meetings.copyMarkdown')}
        </Button>
        <MenuButton
          variant="quiet"
          size="sm"
          icon="language"
          iconEnd="chevron-down"
          disabled={!summaryDone}
          aria-label={t('meetings.summaryLanguage')}
          menuLabel={t('meetings.summaryLanguage')}
          items={SUMMARY_LANGS.map((l) => ({
            key: l,
            label: t(LANG_NAME[l]),
            checked: summaryDone && l === current,
            onSelect: () => {
              if (l !== current) run('lang', () => resummarizeMeeting(m.id, l));
            },
          }))}
        >
          {m.summaryLang ? t(LANG_NAME[current]) : t('meetings.summaryLanguage')}
        </MenuButton>
      </div>
    ) : null;

  let body: ReactNode;
  if (m.state !== 'done') {
    body = <p className="caption hm-muted mt-m0">{t('meetings.summaryAfterTranscript')}</p>;
  } else if (m.summaryState === 'pending' || m.summaryState === 'running' || m.summaryState === 'none') {
    body = (
      <div className="mt-sp__msg" role="status">
        <span className="hm-skeleton__lines" aria-hidden>
          <span style={{ width: '90%' }} />
          <span style={{ width: '70%' }} />
          <span style={{ width: '80%' }} />
        </span>
        <p className="caption hm-muted">{t('meetings.summarizing')}</p>
      </div>
    );
  } else if (m.summaryState === 'skipped' && m.summaryError === 'noModel') {
    body = (
      <div className="mt-sp__msg">
        <p className="caption hm-muted">{t('meetings.summary.noModel')}</p>
        <a className="hm-link" href="#/integrations">{t('integrations.title')}</a>
      </div>
    );
  } else if (m.summaryState === 'skipped' && m.summaryError && SKIPPED[m.summaryError]) {
    body = <p className="caption hm-muted mt-m0">{t(SKIPPED[m.summaryError])}</p>;
  } else if (!summaryDone) {
    body = (
      <div className="mt-sp__msg">
        <p className="caption hm-muted">{t('meetings.summary.failed')}</p>
        <Button variant="secondary" size="sm" disabled={busy === 'summarize'} onClick={() => run('summarize', () => summarizeMeeting(m.id))}>
          {t('meetings.summarize')}
        </Button>
      </div>
    );
  } else {
    body = (
      <>
        {m.summary.length ? (
          <Section title={t('meetings.summary')}>
            <ul>{m.summary.map((s, i) => <li key={i}>{s}</li>)}</ul>
          </Section>
        ) : null}
        {m.decisions.length ? (
          <Section title={t('meetings.decisions')}>
            <ul>{m.decisions.map((s, i) => <li key={i}>{s}</li>)}</ul>
          </Section>
        ) : null}
        {m.actions.length ? (
          <Section title={m.tasksAdded ? t('meetings.actionsAdded', { n: m.tasksAdded }) : t('meetings.actions')}>
            <ul>
              {m.actions.map((a) => {
                const owner = ownerOf(a, m, t);
                const text = `${owner ? `${owner} — ` : ''}${a.text}${a.due ? ` · ${formatDay(a.due, lang)}` : ''}`;
                return (
                  <li key={a.id}>
                    {a.taskId ? (
                      <a className="hm-link" href={`#/tasks/open/${encodeURIComponent(a.taskId)}`}>{text}</a>
                    ) : (
                      <>
                        {text}
                        <Button variant="quiet" size="sm" disabled={busy === a.id} onClick={() => run(a.id, () => addActionToTasks(m.id, a.id))}>
                          {t('meetings.addToTasks')}
                        </Button>
                      </>
                    )}
                  </li>
                );
              })}
            </ul>
          </Section>
        ) : null}
        {m.questions.length ? (
          <Section title={t('meetings.questions')}>
            <ul>{m.questions.map((s, i) => <li key={i}>{s}</li>)}</ul>
          </Section>
        ) : null}
      </>
    );
  }

  const changeText = (c: TaskChange, task: string): string => {
    switch (c.field) {
      case 'due':
        return t('meetings.change.due', {
          task,
          from: c.from ? formatDay(c.from, lang) : t('meetings.change.noDate'),
          to: c.to ? formatDay(c.to, lang) : t('meetings.change.noDate'),
        });
      case 'title':
        return t('meetings.change.rename', { from: c.from ?? task, to: c.to ?? '' });
      case 'done':
        return t('meetings.change.done', { task });
    }
  };

  return (
    <aside className="mt-sp" aria-label={t('meetings.summary')}>
      {bar}
      {body}
      {m.state === 'done'
        ? m.changes.map((c, i) => (
            <div key={c.id} className="mt-chg">
              <span>
                <b>{t('meetings.change.title')}</b>
                <br />
                {changeText(c, tasks[i]?.data?.title ?? '…')}
              </span>
              {c.state === 'pending' ? (
                <div className="mt-chg__row">
                  <Button
                    variant="secondary"
                    size="sm"
                    icon="check"
                    disabled={busy === c.id}
                    onClick={() => run(c.id, () => applyMeetingChange(m.id, c.id))}
                  >
                    {t('meetings.change.apply')}
                  </Button>
                  <Button variant="quiet" size="sm" disabled={busy === c.id} onClick={() => run(c.id, () => ignoreMeetingChange(m.id, c.id))}>
                    {t('meetings.change.ignore')}
                  </Button>
                </div>
              ) : c.state === 'applied' ? (
                <div className="mt-chg__row caption">
                  <span className="hm-muted">{t('meetings.change.applied')}</span>
                  <button
                    type="button"
                    className="hm-link"
                    disabled={busy === c.id}
                    onClick={() => run(c.id, () => undoMeetingChange(m.id, c.id))}
                  >
                    {t('common.undo')}
                  </button>
                </div>
              ) : (
                <span className="caption hm-muted">{t('meetings.change.ignored')}</span>
              )}
            </div>
          ))
        : null}
    </aside>
  );
}
