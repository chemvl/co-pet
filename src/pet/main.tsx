// TEMPORARY Step 4 probe (replaced by the Pet UI): an 80 px disc at the pet's edge point.
// It reports the disc as the pet hit region, logs `pointer-region`, and a click moves the pet to
// the next edge through commit_pet_position.
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { commitPetPosition, getPlacement, on, setHitRegions } from '../shared/ipc';
import type { Edge, Placement } from '../shared/types';

const R = 40;
const NEXT: Record<Edge, Edge> = { right: 'left', left: 'bottom', bottom: 'right' };

function center(p: Placement) {
  const { w, h } = p.workArea;
  if (p.edge === 'left') return { x: R, y: p.offset * h };
  if (p.edge === 'bottom') return { x: p.offset * w, y: h - R };
  return { x: w - R, y: p.offset * h };
}

function Probe() {
  const [p, setP] = useState<Placement | null>(null);
  const [region, setRegion] = useState<string | null>(null);
  useEffect(() => {
    getPlacement().then(setP);
    const offs = [
      on('pet-placement', setP),
      on('pointer-region', (e) => {
        console.log('[probe] pointer-region', e);
        setRegion(e.region);
      }),
    ];
    return () => offs.forEach((o) => o.then((f) => f()));
  }, []);
  useEffect(() => {
    if (!p) return;
    const c = center(p);
    setHitRegions([{ x: c.x - R, y: c.y - R, w: 2 * R, h: 2 * R, kind: 'pet' }]);
  }, [p]);
  if (!p) return null;
  const c = center(p);
  return (
    <div
      onClick={() => commitPetPosition(NEXT[p.edge], 0.3)}
      style={{
        position: 'absolute', left: c.x - R, top: c.y - R, width: 2 * R, height: 2 * R,
        borderRadius: '50%', background: region ? '#f4a27c' : '#9a8f86',
      }}
    />
  );
}

createRoot(document.getElementById('root')!).render(<Probe />);
