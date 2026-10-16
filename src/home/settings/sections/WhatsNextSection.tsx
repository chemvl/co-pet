import { Button, Switch } from '../../../ds';
import { formatWeekday } from '../../../shared/format';
import { useLang, useT } from '../../../shared/i18n';
import { useSetSetting, useSetting } from '../../../shared/queries';
import { Group, SettingRow } from '../parts';

// A type alias (not an interface) so it is assignable to `Json`.
/** `days` are ISO weekdays (1 = Monday … 7 = Sunday), as the scheduler reads them. */
type WorkHours = { start: string; end: string; days: number[] };

const TIME = /^\d{2}:\d{2}$/;
const AWAY_MINUTES = [5, 10, 15, 30];
const PER_HOUR = [1, 2, 3, 4];
const WEEKDAYS = [1, 2, 3, 4, 5, 6, 7];

export function WhatsNextSection() {
  const t = useT();
  const lang = useLang();
  const set = useSetSetting();
  const onReturn = useSetting<boolean>('whatsNext.onReturn', true);
  const afterDone = useSetting<boolean>('whatsNext.afterDone', true);
  const idleMinutes = useSetting<number>('whatsNext.idleMinutes', 10);
  const hours = useSetting<WorkHours>('whatsNext.workHours', { start: '09:00', end: '18:00', days: [1, 2, 3, 4, 5] });
  const maxPerHour = useSetting<number>('whatsNext.maxPerHour', 2);

  const update = (patch: Partial<WorkHours>) => {
    const next = { ...hours, ...patch };
    if (TIME.test(next.start) && TIME.test(next.end)) void set('whatsNext.workHours', next);
  };
  const toggleDay = (day: number) =>
    update({ days: hours.days.includes(day) ? hours.days.filter((d) => d !== day) : [...hours.days, day].sort((a, b) => a - b) });

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
      <Group title={t('settings.whatsnext.workHours')} desc={t('settings.whatsnext.caption')}>
        <div className="st-row" style={{ justifyContent: 'flex-start' }}>
          <label className="st-field">
            {t('settings.notifications.from')}
            <input
              type="time"
              className="st-input st-input--time"
              value={hours.start}
              onChange={(e) => update({ start: e.target.value })}
            />
          </label>
          <label className="st-field">
            {t('settings.notifications.to')}
            <input
              type="time"
              className="st-input st-input--time"
              value={hours.end}
              onChange={(e) => update({ end: e.target.value })}
            />
          </label>
        </div>
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
    </>
  );
}
