// Settings (plan Step 12), rendered by the Home shell for `#/settings/<section>`.
// Every control writes through `set_setting` immediately.
import { useEffect, useRef, type ReactNode } from 'react';
import { useT, type I18nKey } from '../../shared/i18n';
import { AboutSection } from './sections/AboutSection';
import { AppearanceSection } from './sections/AppearanceSection';
import { CaptureSection } from './sections/CaptureSection';
import { DataSection } from './sections/DataSection';
import { GeneralSection } from './sections/GeneralSection';
import { NotificationsSection } from './sections/NotificationsSection';
import { PrivacySection } from './sections/PrivacySection';
import { TriageSection } from './sections/TriageSection';
import { WhatsNextSection } from './sections/WhatsNextSection';
import './settings.css';

export type SettingsSectionId =
  | 'general' | 'capture' | 'triage' | 'whatsnext' | 'notifications' | 'privacy' | 'appearance' | 'data' | 'about';

export const SETTINGS_SECTIONS: readonly { id: SettingsSectionId; label: I18nKey }[] = [
  { id: 'general', label: 'settings.general' },
  { id: 'capture', label: 'settings.capture' },
  { id: 'triage', label: 'settings.triage' },
  { id: 'whatsnext', label: 'settings.whatsnext' },
  { id: 'notifications', label: 'settings.notifications' },
  { id: 'privacy', label: 'settings.privacy' },
  { id: 'appearance', label: 'settings.appearance' },
  { id: 'data', label: 'settings.data' },
  { id: 'about', label: 'settings.about' },
];

export interface SettingsPageProps {
  /** Route segment after `settings/`; unknown values show General. */
  section: string;
  /** Navigates Home to a route without the leading `#/`, e.g. `settings/capture`. */
  onNavigate: (route: string) => void;
}

export function SettingsPage({ section, onNavigate }: SettingsPageProps) {
  const t = useT();
  const current = SETTINGS_SECTIONS.find((s) => s.id === section) ?? SETTINGS_SECTIONS[0];
  const contentRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    contentRef.current?.scrollTo({ top: 0 });
  }, [current.id]);

  let body: ReactNode;
  switch (current.id) {
    case 'general': body = <GeneralSection />; break;
    case 'capture': body = <CaptureSection />; break;
    case 'triage': body = <TriageSection />; break;
    case 'whatsnext': body = <WhatsNextSection />; break;
    case 'notifications': body = <NotificationsSection />; break;
    case 'privacy': body = <PrivacySection onNavigate={onNavigate} />; break;
    case 'appearance': body = <AppearanceSection />; break;
    case 'data': body = <DataSection />; break;
    case 'about': body = <AboutSection />; break;
  }

  return (
    <div className="st-root">
      <nav className="st-nav" aria-label={t('settings.sections')}>
        <h1 className="st-nav__title">{t('settings.title')}</h1>
        {SETTINGS_SECTIONS.map((s) => (
          <button
            key={s.id}
            type="button"
            className="st-nav__item"
            aria-current={s.id === current.id ? 'page' : undefined}
            onClick={() => onNavigate(`settings/${s.id}`)}
          >
            {t(s.label)}
          </button>
        ))}
      </nav>
      <div className="st-content" ref={contentRef}>
        <section className="st-inner" aria-label={t(current.label)}>{body}</section>
      </div>
    </div>
  );
}
