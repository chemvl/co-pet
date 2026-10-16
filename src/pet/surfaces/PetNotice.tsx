// Floating notice for pet-side failures (plan Step 5.7): the facts the pet's
// `confused` can't carry. One quiet action at most; closes after 4 s.
import { useEffect } from 'react';
import { Button } from '../../ds';
import { useT } from '../../shared/i18n';
import type { Notice } from '../notices';

export const NOTICE_MS = 4000;

export function PetNotice(p: { notice: Notice; onAction(): void; onClose(): void }) {
  const t = useT();
  const { onClose } = p;
  useEffect(() => {
    const timer = window.setTimeout(onClose, NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [p.notice, onClose]);
  return (
    <div className="pt-notice" role="status">
      <span className="label">{p.notice.message}</span>
      {p.notice.permission ? (
        <Button variant="quiet" size="sm" onClick={p.onAction}>
          {t('common.openSettings')}
        </Button>
      ) : null}
    </div>
  );
}
