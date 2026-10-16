import { useT } from '../../../shared/i18n';
import { useSetSetting, useSetting } from '../../../shared/queries';
import { Group } from '../parts';

// A type alias (not an interface) so it is assignable to `Json`.
type SleepHours = { start: string; end: string };

const TIME = /^\d{2}:\d{2}$/;

export function NotificationsSection() {
  const t = useT();
  const set = useSetSetting();
  const hours = useSetting<SleepHours>('pet.sleepHours', { start: '23:00', end: '07:00' });

  const update = (patch: Partial<SleepHours>) => {
    const next = { ...hours, ...patch };
    if (TIME.test(next.start) && TIME.test(next.end)) void set('pet.sleepHours', next);
  };

  return (
    <>
      <Group title={t('settings.notifications.sleep')} desc={t('settings.notifications.sleepDesc')}>
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
      </Group>
      <Group title={t('settings.notifications.fullscreen')}>
        <p className="st-body">{t('settings.notifications.fullscreenDesc')}</p>
      </Group>
    </>
  );
}
