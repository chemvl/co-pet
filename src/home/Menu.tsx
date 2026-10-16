// Popover menu for Home (sort, Move to, Other, More). Built from tokens: the design
// system has no menu. Opens below its anchor (flips up when there's no room), arrow keys
// move, Enter runs, Esc or a click outside closes.
import {
  useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactElement, type RefObject,
} from 'react';
import { Button, Icon, type ButtonProps, type IconName } from '../ds';
import { categoryColor, categoryIcon, categoryLabel } from '../shared/categories';
import type { TFunction } from '../shared/i18n';
import type { Category, CategoryId } from '../shared/types';

export interface MenuItem {
  key: string;
  label: string;
  icon?: IconName;
  /** extra class on the icon, e.g. `hm-c-design` for category colour */
  iconClass?: string;
  checked?: boolean;
  danger?: boolean;
  onSelect: () => void;
}

const GAP = 4;
const EDGE = 8;

export function Menu(p: {
  anchor: RefObject<HTMLElement | null>;
  items: MenuItem[];
  label: string;
  align?: 'start' | 'end';
  onClose: (refocus: boolean) => void;
}): ReactElement {
  const { anchor, items, label, align = 'start', onClose } = p;
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);

  useLayoutEffect(() => {
    const a = anchor.current?.getBoundingClientRect();
    const m = ref.current;
    if (!a || !m) return;
    const { width, height } = m.getBoundingClientRect();
    const below = a.bottom + GAP;
    const top = below + height + EDGE > window.innerHeight ? Math.max(EDGE, a.top - GAP - height) : below;
    const rawLeft = align === 'end' ? a.right - width : a.left;
    const left = Math.min(Math.max(EDGE, rawLeft), window.innerWidth - width - EDGE);
    setPos({ top, left });
  }, [anchor, align]);

  // Focus once the menu is placed: a `visibility: hidden` element can't take focus.
  const placed = pos != null;
  useEffect(() => {
    if (placed) ref.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
  }, [placed]);

  useEffect(() => {
    const down = (e: PointerEvent) => {
      const target = e.target as Node;
      if (ref.current?.contains(target) || anchor.current?.contains(target)) return;
      onClose(false);
    };
    const blur = () => onClose(false);
    document.addEventListener('pointerdown', down, true);
    window.addEventListener('blur', blur);
    return () => {
      document.removeEventListener('pointerdown', down, true);
      window.removeEventListener('blur', blur);
    };
  }, [anchor, onClose]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])];
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    let next = -1;
    if (e.key === 'ArrowDown') next = (i + 1) % buttons.length;
    else if (e.key === 'ArrowUp') next = (i - 1 + buttons.length) % buttons.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = buttons.length - 1;
    else if (e.key === 'Escape' || e.key === 'Tab') {
      e.preventDefault();
      e.stopPropagation();
      onClose(e.key === 'Escape');
      return;
    }
    if (next >= 0) {
      e.preventDefault();
      buttons[next]?.focus();
    }
    // Keep list shortcuts (J/K, 1/2, E) from reaching the page while the menu is open.
    e.stopPropagation();
  };

  return (
    <div
      ref={ref}
      className="hm-menu"
      role="menu"
      aria-label={label}
      onKeyDown={onKeyDown}
      style={pos ? { top: pos.top, left: pos.left } : { top: 0, left: 0, visibility: 'hidden' }}
    >
      {items.map((it) => (
        <button
          key={it.key}
          type="button"
          role="menuitem"
          className={`hm-menu__item${it.danger ? ' is-danger' : ''}`}
          onClick={() => {
            onClose(true);
            it.onSelect();
          }}
        >
          {it.icon ? <span className={`hm-menu__icon ${it.iconClass ?? ''}`}><Icon name={it.icon} size={18} /></span> : null}
          <span className="hm-menu__label">{it.label}</span>
          {it.checked ? <span className="hm-menu__check"><Icon name="check" size={16} /></span> : null}
        </button>
      ))}
    </div>
  );
}

/** A design-system Button that toggles a Menu. */
export function MenuButton(p: ButtonProps & { items: MenuItem[]; menuLabel: string; align?: 'start' | 'end' }): ReactElement {
  const { items, menuLabel, align, ...buttonProps } = p;
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLSpanElement>(null);
  return (
    <span ref={anchor} className="hm-menu-anchor">
      <Button
        {...buttonProps}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      />
      {open ? (
        <Menu
          anchor={anchor}
          items={items}
          label={menuLabel}
          align={align}
          onClose={(refocus) => {
            setOpen(false);
            if (refocus) anchor.current?.querySelector('button')?.focus();
          }}
        />
      ) : null}
    </span>
  );
}

/** Every live category, each in its own colour; `current` gets a check. */
export function categoryMenuItems(
  t: TFunction,
  categories: readonly Category[],
  onPick: (id: CategoryId) => void,
  current?: CategoryId,
): MenuItem[] {
  return categories.map((c) => ({
    key: c.id,
    label: categoryLabel(c, t),
    icon: categoryIcon(c),
    iconClass: `hm-c-${categoryColor(c)}`,
    checked: c.id === current,
    onSelect: () => onPick(c.id),
  }));
}
