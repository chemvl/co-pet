import { Switch } from '../../../ds';
import { useT } from '../../../shared/i18n';
import { useSetSetting, useSetting } from '../../../shared/queries';
import { Group, TimeRange } from '../parts';

// A type alias (not an interface) so it is assignable to `Json`.
type SleepHours = { start: string; end: string };

export function NotificationsSection() {
  const t = useT();
  const set = useSetSetting();
  const hours = useSetting<SleepHours>('pet.sleepHours', { start: '23:00', end: '07:00' });
  const diaryNotice = useSetting<boolean>('diary.weeklyNotice', true);

  return (
    <>
      <Group title={t('settings.notifications.sleep')} desc={t('settings.notifications.sleepDesc')}>
        <TimeRange start={hours.start} end={hours.end} onChange={(range) => void set('pet.sleepHours', range)} />
      </Group>
      <Group title={t('settings.notifications.fullscreen')}>
        <p className="st-body">{t('settings.notifications.fullscreenDesc')}</p>
      </Group>
      <Group title={t('diary.title')}>
        <Switch
          id="st-diary-notice"
          label={t('settings.notifications.diaryNotice')}
          description={t('settings.notifications.diaryNoticeDesc')}
          checked={diaryNotice}
          onChange={(v) => void set('diary.weeklyNotice', v)}
        />
      </Group>
    </>
  );
}
