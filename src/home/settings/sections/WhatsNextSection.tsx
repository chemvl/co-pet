import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Button, Switch } from '../../../ds';
import { formatWeekday } from '../../../shared/format';
import { useLang, useT } from '../../../shared/i18n';
import { listCalendars, permissionsStatus, requestPermission } from '../../../shared/ipc';
import { useSetSetting, useSetting } from '../../../shared/queries';
import { Group, SettingRow, TimeRange } from '../parts';

// A type alias (not an interface) so it is assignable to `Json`.
/** `days` are ISO weekdays (1 = Monday … 7 = Sunday), as the scheduler reads them. */
type WorkHours = { start: string; end: string; days: number[] };

const AWAY_MINUTES = [5, 10, 15, 30];
const PER_HOUR = [1, 2, 3, 4];
const WEEKDAYS = [1, 2, 3, 4, 5, 6, 7];

export function WhatsNextSection() {
  const t = useT();
  const lang = useLang();
  const set = useSetSetting();
  const onReturn = useSetting<boolean>('whatsNext.onReturn', true);
  const afterDone = useSetting<boolean>('whatsNext.afterDone', true);
  const afterMeeting = useSetting<boolean>('whatsNext.afterMeeting', true);
  const idleMinutes = useSetting<number>('whatsNext.idleMinutes', 10);
  const hours = useSetting<WorkHours>('whatsNext.workHours', { start: '09:00', end: '18:00', days: [1, 2, 3, 4, 5] });
  const maxPerHour = useSetting<number>('whatsNext.maxPerHour', 2);
  const calendarOn = useSetting<boolean>('whatsNext.calendar', false);
  const excluded = useSetting<string[]>('whatsNext.calendarExcluded', []);
  const qc = useQueryClient();
  const perms = useQuery({
    queryKey: ['permissions'],
    queryFn: permissionsStatus,
    enabled: calendarOn,
    refetchInterval: (q) => (q.state.data?.calendar === 'granted' ? false : 2000),
  });
  const access = perms.data?.calendar;
  const cals = useQuery({ queryKey: ['calendars'], queryFn: listCalendars, enabled: calendarOn && access === 'granted' });
  const refreshNext = () => void qc.invalidateQueries({ queryKey: ['next'] });

  const toggleDay = (day: number) =>
    void set('whatsNext.workHours', {
      ...hours,
      days: hours.days.includes(day) ? hours.days.filter((d) => d !== day) : [...hours.days, day].sort((a, b) => a - b),
    });

  return (
    <>
      <Group title={t('settings.whatsnext')}>
        <Switch
          id="st-next-return"
          label={t('settings.whatsnext.onReturn')}
          description={t('settings.whatsnext.onReturnDesc', { n: idleMinutes })}
          checked={onReturn}
          onChange={(v) => void set('whatsNext.onReturn', v)}
        />
        <SettingRow label={t('settings.whatsnext.awayFor')} htmlFor="st-next-away">
          <select
            id="st-next-away"
            className="st-input"
            value={idleMinutes}
            onChange={(e) => void set('whatsNext.idleMinutes', Number(e.currentTarget.value))}
          >
            {AWAY_MINUTES.map((n) => (
              <option key={n} value={n}>
                {t('settings.whatsnext.minutes', { n })}
              </option>
            ))}
          </select>
        </SettingRow>
        <Switch
          id="st-next-done"
          label={t('settings.whatsnext.afterDone')}
          checked={afterDone}
          onChange={(v) => void set('whatsNext.afterDone', v)}
        />
        <Switch
          id="st-next-meeting"
          label={t('settings.whatsnext.afterMeeting')}
          description={t('settings.whatsnext.afterMeetingDesc')}
          checked={afterMeeting}
          onChange={(v) => void set('whatsNext.afterMeeting', v)}
        />
        <SettingRow label={t('settings.whatsnext.maxPerHour')} htmlFor="st-next-rate">
          <select
            id="st-next-rate"
            className="st-input"
            value={maxPerHour}
            onChange={(e) => void set('whatsNext.maxPerHour', Number(e.currentTarget.value))}
          >
            {PER_HOUR.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </SettingRow>
      </Group>
      <Group title={t('settings.whatsnext.workHours')}>
        <TimeRange start={hours.start} end={hours.end} onChange={(range) => void set('whatsNext.workHours', { ...hours, ...range })} />
        <div className="st-row__control" role="group" aria-label={t('settings.whatsnext.days')}>
          {WEEKDAYS.map((day) => {
            const on = hours.days.includes(day);
            return (
              <Button key={day} size="sm" variant={on ? 'secondary' : 'quiet'} aria-pressed={on} onClick={() => toggleDay(day)}>
                {formatWeekday(day, lang)}
              </Button>
            );
          })}
        </div>
      </Group>
      <Group title={t('settings.whatsnext.calendar')}>
        <Switch
          id="st-next-cal"
          label={t('settings.whatsnext.calendarUse')}
          description={t('settings.whatsnext.calendarUseDesc')}
          checked={calendarOn}
          onChange={(v) => {
            void set('whatsNext.calendar', v).then(refreshNext);
            if (v)
              void permissionsStatus().then((p) => {
                if (p.calendar === 'notDetermined') void requestPermission('calendar');
              });
          }}
        />
        {calendarOn && access === 'notDetermined' && (
          <>
            <p className="st-body">{t('settings.whatsnext.calendarAsk')}</p>
            <Button size="sm" variant="secondary" onClick={() => void requestPermission('calendar')}>
              {t('settings.whatsnext.calendarAllow')}
            </Button>
          </>
        )}
        {calendarOn && access === 'denied' && (
          <>
            <p className="st-body">{t('settings.whatsnext.calendarDenied')}</p>
            <Button size="sm" variant="secondary" onClick={() => void requestPermission('calendar')}>
              {t('onboarding.perm.open')}
            </Button>
          </>
        )}
        {calendarOn && access === 'granted' && (
          <>
            <p className="st-body">{t('settings.whatsnext.calendars')}</p>
            {cals.data?.length === 0 && <p className="st-body">{t('settings.whatsnext.calendarsEmpty')}</p>}
            {cals.data?.map((c, i) => (
              <Switch
                key={c.id}
                id={`st-next-cal-${i}`}
                label={c.title}
                description={c.account || undefined}
                checked={!excluded.includes(c.id)}
                onChange={(on) =>
                  void set('whatsNext.calendarExcluded', on ? excluded.filter((id) => id !== c.id) : [...excluded, c.id]).then(
                    refreshNext,
                  )
                }
              />
            ))}
          </>
        )}
      </Group>
      <p className="st-body">{t('settings.whatsnext.caption')}</p>
    </>
  );
}
