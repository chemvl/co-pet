// Right-click menu (plan Step 5.5). Built from tokens: the design system has no menu.
import { useEffect, useRef, type KeyboardEvent } from 'react';
import { useT } from '../../shared/i18n';

export interface PetMenuEntry {
  key: string;
  label: string;
  hint?: string;
  onSelect(): void;
}

/** `null` entries render as separators. `label` names the menu (default: the pet menu). */
export function PetMenu(p: { items: (PetMenuEntry | null)[]; label?: string; onClose(): void }) {
  const t = useT();
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // Next frame: the right-click that opened the menu focuses the pet after this commit.
    const raf = requestAnimationFrame(() =>
      ref.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus(),
    );
    return () => cancelAnimationFrame(raf);
  }, []);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const items = [...(ref.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])];
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    let next = -1;
    if (e.key === 'ArrowDown') next = (at + 1) % items.length;
    else if (e.key === 'ArrowUp') next = (at - 1 + items.length) % items.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = items.length - 1;
    else if (e.key === 'Escape') {
      e.preventDefault();
      p.onClose();
      return;
    }
    if (next >= 0) {
      e.preventDefault();
      items[next]?.focus();
    }
  };

  return (
    <div ref={ref} className="pt-menu" role="menu" aria-label={p.label ?? t('pet.menu.label')} onKeyDown={onKeyDown}>
      {p.items.map((item, i) =>
        item ? (
          <button
            key={item.key}
            type="button"
            role="menuitem"
            className="pt-menu__item label"
            onClick={() => {
              p.onClose();
              item.onSelect();
            }}
          >
            <span>{item.label}</span>
            {item.hint ? <span className="pt-menu__hint caption">{item.hint}</span> : null}
          </button>
        ) : (
          <div key={`sep-${i}`} role="separator" className="pt-menu__sep" />
        ),
      )}
    </div>
  );
}
