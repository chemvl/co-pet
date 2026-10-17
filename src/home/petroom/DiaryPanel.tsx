// Pet diary: one week of items saved, tasks done and meetings, computed live by `diary_week`.
import { useQuery } from '@tanstack/react-query';
import { useState, type ReactElement } from 'react';
import { Button } from '../../ds';
import { formatDate, formatDuration, formatWeekday, formatWeekRange } from '../../shared/format';
import { useLang, useT } from '../../shared/i18n';
import { useSetting } from '../../shared/queries';
import { CategoryTag } from '../CategoryTag';
import { useNow } from '../hooks';
import { EmptyState, QueryState } from '../InboxPage';
import { diaryQuery } from '../queries';
import { navigate } from '../router';
import { dayBars, isEmptyWeek, todayIso, weekHeading } from './diary';
import './diary.css';

const cx = (...parts: (string | false)[]) => parts.filter(Boolean).join(' ');

function localDate(iso: string): Date {
  const [y, m, d] = iso.split('-').map(Number);
  return new Date(y, m - 1, d);
}

export function DiaryPanel(): ReactElement {
  const t = useT();
  const lang = useLang();
  const now = useNow();
  const name = useSetting<string>('pet.name', 'Miso') || 'Miso';
  const [offset, setOffset] = useState<number | null>(null);
  const q = useQuery(diaryQuery(offset));
  const w = q.data;
  const year = new Date(now).getFullYear();

  let body: ReactElement;
  if (!w) {
    body = <QueryState q={q} loading={t('diary.loading')} />;
  } else if (isEmptyWeek(w)) {
    body = w.offset === 0 ? (
      <EmptyState
        title={t('diary.empty.thisWeek')}
        line={t('diary.empty.thisWeekLine', { name })}
        action={{ label: t('diary.empty.openInbox'), run: () => navigate('inbox') }}
      />
    ) : (
      <EmptyState
        title={t('diary.empty.past')}
        line={t('diary.empty.pastLine')}
        action={{ label: t('diary.empty.showThisWeek'), run: () => setOffset(0) }}
      />
    );
  } else {
    body = (
      <>
        <dl className="dy__stats">
          <div className="dy__stat">
            <dt className="caption">{t('diary.saved')}</dt>
            <dd className="title">{w.captured}</dd>
          </div>
          <div className="dy__stat">
            <dt className="caption">{t('diary.done')}</dt>
            <dd className="title">{w.tasksDone}</dd>
          </div>
          <div className="dy__stat">
            <dt className="caption">{t('diary.meetings')}</dt>
            <dd className="title">{w.meetings}</dd>
            {w.meetings > 0 ? <dd className="caption">{formatDuration(w.meetingMs, lang)}</dd> : null}
          </div>
        </dl>
        <ol className="dy__days" aria-label={t('diary.days')}>
          {dayBars(w.days, todayIso(new Date(now))).map((b, i) => {
            const d = w.days[i];
            return (
              <li
                key={b.date}
                className={cx('dy__day', b.future && 'is-future')}
                aria-label={t('diary.day', {
                  day: formatDate(localDate(b.date), lang),
                  saved: d.captured,
                  done: d.tasksDone,
                  meetings: d.meetings,
                })}
              >
                <span className="dy__track" aria-hidden>
                  <span className={cx('dy__bar', b.total === 0 && 'is-zero')} style={{ blockSize: `${b.pct}%` }} />
                </span>
                <span className="caption" aria-hidden>{formatWeekday(b.isoDay, lang)}</span>
              </li>
            );
          })}
        </ol>
        {w.topCategories.length > 0 ? (
          <div className="dy__cats">
            <span className="caption">{t('diary.topCategories')}</span>
            {w.topCategories.map((c) => (
              <CategoryTag key={c.categoryId} id={c.categoryId} size="sm" count={c.count} />
            ))}
          </div>
        ) : null}
        <p className="caption dy__foot">{t('diary.footer')}</p>
      </>
    );
  }

  return (
    <section className="dy" aria-labelledby="dy-title">
      <header className="dy__head">
        <h2 id="dy-title" className="heading">{t('diary.title')}</h2>
        {w ? (
          <div className="dy__nav">
            <Button variant="quiet" size="sm" disabled={!w.hasPrevious} onClick={() => setOffset(w.offset - 1)}>
              {t('diary.earlier')}
            </Button>
            <span className="body-strong" aria-live="polite">{weekHeading(w, t, lang, year)}</span>
            <Button variant="quiet" size="sm" disabled={w.offset === 0} onClick={() => setOffset(w.offset + 1)}>
              {t('diary.later')}
            </Button>
          </div>
        ) : null}
      </header>
      {w && w.offset >= -1 ? <p className="caption dy__range">{formatWeekRange(w.start, w.end, lang, year)}</p> : null}
      {body}
    </section>
  );
}
