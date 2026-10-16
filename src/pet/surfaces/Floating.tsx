// Positions one pet-side surface: measures it, asks `place` for its top-left, and
// reports the resulting rect so it can take the pointer (hit region `panel`).
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import type { Box, Point, Size } from '../placement';

export function Floating(p: {
  place: (size: Size) => Point;
  onRect: (rect: Box | null) => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState<Size | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const r = el.getBoundingClientRect();
      setSize((s) => (s && s.w === r.width && s.h === r.height ? s : { w: r.width, h: r.height }));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const pos = size ? p.place(size) : null;
  const { onRect } = p;
  const x = pos?.x;
  const y = pos?.y;
  useLayoutEffect(() => {
    if (x === undefined || y === undefined || !size) return;
    onRect({ x, y, w: size.w, h: size.h });
  }, [x, y, size, onRect]);
  useLayoutEffect(() => () => onRect(null), [onRect]);

  return (
    <div
      ref={ref}
      className="pt-floating"
      // Unmeasured: invisible but still focusable, so autofocus inside works.
      style={pos ? { left: pos.x, top: pos.y } : { left: 0, top: 0, opacity: 0 }}
    >
      {p.children}
    </div>
  );
}
