// First recording only: a reminder to tell the others, before anything is captured.
// Start records the consent and starts; Cancel or Esc closes.
import { Button } from '../../ds';
import { useT } from '../../shared/i18n';

export function ConsentPanel(p: { onStart(): void; onClose(): void }) {
  const t = useT();
  return (
    <section
      className="pt-consent"
      aria-labelledby="pt-consent-title"
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          p.onClose();
        }
      }}
    >
      <h2 id="pt-consent-title" className="heading">{t('rec.consent.title')}</h2>
      <p className="body">{t('rec.consent.body')}</p>
      <div className="pt-consent__actions">
        <Button variant="quiet" size="sm" onClick={p.onClose}>
          {t('common.cancel')}
        </Button>
        <Button variant="primary" size="sm" icon="mic" autoFocus onClick={p.onStart}>
          {t('rec.consent.start')}
        </Button>
      </div>
    </section>
  );
}
