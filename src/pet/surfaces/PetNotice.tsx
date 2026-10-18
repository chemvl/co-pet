// Floating notice for pet-side failures (plan Step 5.7), call offers and auto-stops: the facts
// the pet's `confused` can't carry. One quiet action at most; closes after `notice.ms` (4 s by default).
import { useEffect } from 'react';
import { Button } from '../../ds';
import { useT } from '../../shared/i18n';
import { NOTICE_MS, type Notice } from '../notices';

export function PetNotice(p: { notice: Notice; onAction(): void; onClose(): void }) {
  const t = useT();
  const { onClose } = p;
  useEffect(() => {
    const timer = window.setTimeout(onClose, p.notice.ms ?? NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [p.notice, onClose]);
  const n = p.notice;
  const action = n.permission
    ? t('common.openSettings')
    : n.transcribeItemId
      ? t('notice.transcribe')
      : n.home !== undefined
        ? (n.homeLabel ?? t('common.openHome'))
        : n.record
          ? t('notice.record')
          : n.keepRecording
            ? t('notice.keepRecording')
            : n.stopRecording
              ? t('notice.stopRecording')
              : null;
  return (
    <div className="pt-notice" role="status">
      <span className="label">{n.message}</span>
      {action ? (
        <Button variant="quiet" size="sm" onClick={p.onAction}>
          {action}
        </Button>
      ) : null}
    </div>
  );
}
