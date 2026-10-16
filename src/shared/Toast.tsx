import { createContext, useCallback, useContext, useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { Button } from '../ds';
import { useT } from './i18n';

export interface ToastOptions {
  message: string;
  action?: { label: string; run: () => void };
  undo?: () => void;
}
type ShowToast = (opts: ToastOptions) => void;

const AUTO_DISMISS_MS = 6000;

const ToastContext = createContext<ShowToast>(() => {
  throw new Error('useToast() requires <ToastProvider>');
});

const toastStyle: CSSProperties = {
  position: 'fixed',
  right: 'var(--space-6)',
  bottom: 'var(--space-6)',
  zIndex: 1000,
  display: 'flex',
  alignItems: 'center',
  gap: 'var(--space-2)',
  maxWidth: 480,
  padding: 'var(--space-2) var(--space-2) var(--space-2) var(--space-4)',
  background: 'var(--surface-raised)',
  border: '1px solid var(--line)',
  borderRadius: 'var(--radius-md)',
  boxShadow: 'var(--shadow-sm)',
  color: 'var(--ink)',
};

/** One toast at a time, bottom-right of the content area; a new toast replaces the current one. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const t = useT();
  const [toast, setToast] = useState<(ToastOptions & { id: number }) | null>(null);
  const seq = useRef(0);

  const show = useCallback<ShowToast>((opts) => {
    seq.current += 1;
    setToast({ ...opts, id: seq.current });
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), AUTO_DISMISS_MS);
    return () => window.clearTimeout(timer);
  }, [toast]);

  return (
    <ToastContext.Provider value={show}>
      {children}
      <div role="status" aria-live="polite">
        {toast ? (
          <div key={toast.id} style={toastStyle}>
            <span className="label" style={{ flex: 1 }}>{toast.message}</span>
            {toast.action ? (
              <Button variant="quiet" size="sm" onClick={() => { setToast(null); toast.action!.run(); }}>
                {toast.action.label}
              </Button>
            ) : null}
            {toast.undo ? (
              <Button variant="quiet" size="sm" icon="undo" onClick={() => { setToast(null); toast.undo!(); }}>
                {t('common.undo')}
              </Button>
            ) : null}
          </div>
        ) : null}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): { show: ShowToast } {
  const show = useContext(ToastContext);
  return { show };
}
