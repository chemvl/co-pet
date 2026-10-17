// The pet window (plan Steps 5.3–5.7, 6, 7, 10): a full-window transparent root
// holding the design-system PetDock with the cat, its bubbles and one transient
// surface at a time. Only the pet, the bubbles and open panels take the pointer
// (hit regions → Rust); everything else clicks through.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore,
  type CSSProperties, type KeyboardEvent, type PointerEvent as ReactPointerEvent,
} from 'react';
import { PetDock, type IconName } from '../ds';
import { categoryColor, categoryIcon, categoryLabelOf } from '../shared/categories';
import { errorMessage, errorText } from '../shared/errors';
import { useT, type I18nKey } from '../shared/i18n';
import {
  captureClipboard, captureNote, captureScreenshot, captureSelection, commitPetPosition,
  counts as fetchCounts, focusPetPanel, getPlacement, nextAction, on, openHome, pasteItems, pauseRecording, petStatus,
  quitApp, requestPermission, resignPetPanel, resumeRecording, setHitRegions, setPetMoving, setRecordingLanguage,
  startRecording, stopRecording, transcribeItem, whatsNext,
} from '../shared/ipc';
import { useCategories, useRecording, useSetSetting, useSettings } from '../shared/queries';
import type {
  CaptureResult, EventPayloads, HitRegion, HotkeyAction, MeetingLanguage, NextCard, NextTrigger, Placement,
  RecordingStatus,
} from '../shared/types';
import { asPetSize, petLookOf, type PetSize, type SkinId, type Species } from './catalog';
import { CatArt } from './CatArt';
import { noticeFor, type Notice } from './notices';
import {
  bubbleRects, bubbleZone, dockBox, edgePoint, petBox, petHitRect, placeAtPointer, placePanel, snapToEdge,
  DOCK_BOX, type Box, type Point, type Size,
} from './placement';
import { fxForArt, poseFor } from './pose';
import { Floating } from './surfaces/Floating';
import { ConsentPanel } from './surfaces/ConsentPanel';
import { PetMenu } from './surfaces/PetMenu';
import type { CardAction } from '../shared/NextCardView';
import { NextPanel } from './surfaces/NextPanel';
import { PetNotice } from './surfaces/PetNotice';
import { QuickNotePanel } from './surfaces/QuickNotePanel';
import { RecordingDock } from './surfaces/RecordingDock';
import { COUNTS_KEY, TRAY_KEY, TrayPanel } from './surfaces/TrayPanel';
import { usePetLook } from './usePetMachine';
import './pet.css';

type Surface = null | 'bubbles' | 'tray' | 'menu' | 'quicknote' | 'notice' | 'next' | 'consent' | 'recLang';
type Region = 'pet' | 'panel' | 'halo' | 'zone' | null;

const BUBBLES: { icon: IconName; label: I18nKey }[] = [
  { icon: 'whats-next', label: 'pet.bubble.whatsNext' },
  { icon: 'screenshot', label: 'pet.bubble.captureScreen' },
  { icon: 'clipboard', label: 'pet.bubble.saveClipboard' },
  { icon: 'mic', label: 'pet.bubble.recordMeeting' },
  { icon: 'home', label: 'pet.bubble.openHome' },
  { icon: 'note', label: 'pet.bubble.quickNote' },
];

/** Pointer rests this long on the pet before the bubbles fan out. */
const REST_MS = 150;
/** Bubbles fold this long after the pointer leaves. */
const FOLD_MS = 400;
/** Back to peek this long after the pointer leaves with nothing open… */
const PEEK_MS = 3000;
/** …or this long after a drag passes by. */
const PEEK_AFTER_DRAG_MS = 1000;
/** Halo fade-out (`dur-fast`). */
const HALO_FADE_MS = 120;
const DOUBLE_CLICK_MS = 300;
const MOVE_THRESHOLD = 4;
const HIDE_HOUR_MS = 3_600_000;
/** Surfaces that need keyboard focus: the panel is made key while they're open. */
const KEYED: Surface[] = ['tray', 'menu', 'quicknote', 'consent', 'recLang'];
/** The HUD shows only beside these; any other surface opens over it. */
const WITH_HUD: Surface[] = [null, 'bubbles', 'recLang'];
const REC_LANGUAGES: { lang: MeetingLanguage; label: I18nKey }[] = [
  { lang: 'auto', label: 'lang.auto' },
  { lang: 'en', label: 'lang.en' },
  { lang: 'vi', label: 'lang.vi' },
  { lang: 'zh', label: 'lang.zh' },
  { lang: 'vi+en', label: 'lang.viEn' },
];

interface Look { species: Species; skin: SkinId; size: PetSize }

const reducedMotionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
function subscribeReducedMotion(cb: () => void) {
  reducedMotionQuery.addEventListener('change', cb);
  return () => reducedMotionQuery.removeEventListener('change', cb);
}

export function PetApp() {
  const t = useT();
  const qc = useQueryClient();
  const setSetting = useSetSetting();
  const { data: settings } = useSettings();
  const reducedMotion = useSyncExternalStore(subscribeReducedMotion, () => reducedMotionQuery.matches);

  // ---------- settings → look, name, paused ----------
  const name = typeof settings?.['pet.name'] === 'string' && settings['pet.name'] ? settings['pet.name'] : 'Miso';
  const paused = settings?.['pet.paused'] === true;
  const wanted: Look | null = settings ? { ...petLookOf(settings), size: asPetSize(settings['pet.size']) } : null;
  const wantedKey = wanted ? `${wanted.species}/${wanted.skin}/${wanted.size}` : null;
  const { machine, output, applied } = usePetLook(wanted, wantedKey);

  useEffect(() => machine.setPaused(paused), [machine, paused]);

  // ---------- window state ----------
  const [placement, setPlacement] = useState<Placement | null>(null);
  const [hidden, setHidden] = useState(false);
  const [region, setRegion] = useState<Region>(null);
  /** `id` of the hit region under the pointer (a bubble's icon), from the native hit test. */
  const [hoverTarget, setHoverTarget] = useState<string | null>(null);
  const [out, setOut] = useState(false);
  const [surface, setSurface] = useState<Surface>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [menuAt, setMenuAt] = useState<Point | null>(null);
  const [panelRect, setPanelRect] = useState<Box | null>(null);
  const [hudRect, setHudRect] = useState<Box | null>(null);
  const [halo, setHalo] = useState<'none' | 'on' | 'fading'>('none');
  const [moving, setMoving] = useState<Point | null>(null);
  const [petFocused, setPetFocused] = useState(false);
  const [focusIndex, setFocusIndex] = useState<number | undefined>(undefined);
  const [announcement, setAnnouncement] = useState('');
  const [next, setNext] = useState<{ card: NextCard; trigger: NextTrigger; userInitiated: boolean } | null>(null);

  const petRef = useRef<HTMLButtonElement>(null);
  const bubbleEls = useRef<(HTMLButtonElement | null)[]>([]);
  const dragOutActive = useRef(false);
  const peekDelay = useRef(PEEK_MS);
  const suppressClick = useRef(false);
  const clickTimer = useRef<number | null>(null);
  const restTimer = useRef<number | null>(null);
  const foldTimer = useRef<number | null>(null);
  const haloTimer = useRef<number | null>(null);

  // Latest values for event handlers subscribed once. Handlers also write the
  // fields they change, so back-to-back events see them before the next render.
  const live = useRef({ surface, region, halo, placement, hidden });
  live.current = { surface, region, halo, placement, hidden };

  const clear = (timer: { current: number | null }) => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
  };

  const { data: counts } = useQuery({ queryKey: COUNTS_KEY, queryFn: fetchCounts });
  const unsorted = counts?.unsorted ?? 0;

  // ---------- surfaces ----------
  // The What's Next card holds the surface until one of its actions closes it (`onNextAction`).
  const openSurface = useCallback((next: Exclude<Surface, null>) => {
    if (live.current.surface === 'next') return;
    live.current.surface = next;
    setSurface(next);
    setOut(true);
    if (KEYED.includes(next)) void focusPetPanel();
  }, []);

  const closeSurface = useCallback(() => {
    if (live.current.surface === 'next') return;
    live.current.surface = null;
    setSurface(null);
    setFocusIndex(undefined);
    void resignPetPanel();
  }, []);

  // A notice raised while the What's Next card is up waits until the card closes.
  const pendingNotice = useRef<Notice | null>(null);
  const showNotice = useCallback(
    (n: Notice) => {
      if (live.current.surface === 'next') {
        pendingNotice.current = n;
        return;
      }
      setNotice(n);
      openSurface('notice');
    },
    [openSurface],
  );

  const announce = (text: string) => setAnnouncement(text);

  const { data: categories } = useCategories();

  const handleResult = useCallback(
    (r: CaptureResult) => {
      if (r.status === 'duplicate') announce(t('pet.announce.duplicate'));
      const n = noticeFor(r, t);
      if (!n) return;
      // Rust plays `confused` for failures; a skip with something to say gets it here.
      if (r.status === 'skipped') machine.fire('confused');
      showNotice(n);
    },
    [machine, showNotice, t],
  );

  const runCapture = useCallback(
    (capture: () => Promise<CaptureResult>) => {
      capture().then(handleResult, (err: unknown) => {
        machine.fire('confused');
        showNotice({ message: t('notice.captureFailed', { reason: String(err) }) });
      });
    },
    [handleResult, machine, showNotice, t],
  );

  const paste = useCallback(
    (ids: string[]) => {
      closeSurface();
      pasteItems(ids).then(
        (r) => {
          if (r === 'needsAccessibility') {
            showNotice({ message: t('notice.needsAccessibility'), permission: 'accessibility' });
          }
        },
        (err: unknown) => {
          // Known codes (a file on another Mac) have their own copy.
          const message = errorMessage(t, err);
          showNotice({ message: message === errorText(err) ? t('notice.captureFailed', { reason: message }) : message });
        },
      );
    },
    [closeSurface, showNotice, t],
  );

  const goHome = useCallback(
    (route?: string) => {
      closeSurface();
      void openHome(route);
    },
    [closeSurface],
  );

  // ---------- What's Next ----------
  const shownCard = useRef<NextCard | null>(null);

  /** Closes the card and shows a notice that waited behind it. */
  const closeNext = () => {
    live.current.surface = null;
    shownCard.current = null;
    setSurface(null);
    void resignPetPanel();
    const pending = pendingNotice.current;
    pendingNotice.current = null;
    if (pending) showNotice(pending);
  };

  /** User-initiated cards take keyboard focus; proactive ones never do. */
  const openNext = (card: NextCard, trigger: NextTrigger, userInitiated: boolean) => {
    setNext({ card, trigger, userInitiated });
    shownCard.current = card;
    live.current.surface = 'next';
    setSurface('next');
    setOut(true);
    if (userInitiated) void focusPetPanel();
    nextAction(card.taskId, 'shown', trigger).catch((e: unknown) => {
      // The task is gone since it was picked: take the card down without recording anything.
      if (errorText(e) === 'notFound' && shownCard.current === card && live.current.surface === 'next') closeNext();
    });
  };

  const onNextAction = (action: CardAction) => {
    if (!next || live.current.surface !== 'next') return;
    closeNext();
    nextAction(next.card.taskId, action, next.trigger).catch((e: unknown) =>
      showNotice({ message: errorMessage(t, e) }),
    );
  };

  const askNext = async () => {
    closeSurface();
    machine.set('thinking', true);
    let card: NextCard | null;
    try {
      card = await whatsNext('bubble');
    } catch (e) {
      showNotice({ message: errorMessage(t, e) });
      return;
    } finally {
      machine.set('thinking', false);
    }
    // Stale pick: the pet was hidden, or the user (or a proactive card) took a surface meanwhile.
    const now = live.current.surface;
    if (live.current.hidden || (now !== null && now !== 'bubbles')) return;
    if (card) openNext(card, 'bubble', true);
    else showNotice({ message: t('notice.nextEmpty'), home: 'tasks' });
  };

  // The pet holds up its sign while the card is open.
  useEffect(() => machine.set('holdSign', surface === 'next'), [machine, surface]);

  // ---------- Record meeting ----------
  const { data: recordingData, dataUpdatedAt: recordingAt } = useRecording();
  const recState = recordingData?.state ?? 'idle';
  const consentShown = settings?.['transcribe.consentShown'] === true;

  const startNow = () => {
    startRecording().then(
      (s) => {
        if (!s.mic) showNotice({ message: t('notice.recMicMissing'), permission: 'microphone' });
        else if (!s.system) showNotice({ message: t('notice.recSystemMissing'), permission: 'screenRecording' });
      },
      (e: unknown) =>
        showNotice({
          message: errorMessage(t, e),
          permission: errorText(e) === 'noAudioSource' ? 'microphone' : undefined,
        }),
    );
  };

  /** Bubble `mic` and hotkey `recordMeeting`: a running recording only wakes the pet. */
  const beginRecording = () => {
    if (recState !== 'idle') {
      machine.fire('wake');
      setOut(true);
      return;
    }
    if (!consentShown) {
      openSurface('consent');
      return;
    }
    closeSurface();
    startNow();
  };

  const onConsentStart = () => {
    void setSetting('transcribe.consentShown', true);
    closeSurface();
    startNow();
  };

  const onHudPause = () => {
    (recState === 'recording' ? pauseRecording : resumeRecording)().catch((e: unknown) =>
      showNotice({ message: errorMessage(t, e) }),
    );
  };

  const onHudStop = () => {
    stopRecording().catch((e: unknown) => {
      // `recording-status` (endedReason `tooShort`) already says it.
      if (errorText(e) !== 'tooShort') showNotice({ message: errorMessage(t, e) });
    });
  };

  /** The language menu opens at the HUD's language button, away from the edge. */
  const onHudLanguage = (anchor: HTMLButtonElement) => {
    const r = anchor.getBoundingClientRect();
    const edgeNow = live.current.placement?.edge ?? 'right';
    const x = edgeNow === 'right' ? r.right : edgeNow === 'left' ? r.left : r.left + r.width / 2;
    setMenuAt({ x, y: edgeNow === 'bottom' ? r.top : r.bottom });
    openSurface('recLang');
  };

  const onRecordingStatus = (s: RecordingStatus) => {
    if (s.state !== 'idle' || !s.endedReason) return;
    showNotice({ message: t(s.endedReason === 'maxLength' ? 'notice.recMaxLength' : 'notice.recTooShort') });
  };

  // The language menu belongs to the HUD: it goes when the recording ends.
  useEffect(() => {
    if (recState === 'idle' && surface === 'recLang') closeSurface();
  }, [recState, surface, closeSurface]);

  const onBubble = (icon: IconName) => {
    if (icon === 'whats-next') {
      void askNext();
      return;
    }
    if (icon === 'note') {
      openSurface('quicknote');
      return;
    }
    if (icon === 'mic') {
      beginRecording();
      return;
    }
    closeSurface();
    if (icon === 'screenshot') runCapture(captureScreenshot);
    else if (icon === 'clipboard') runCapture(captureClipboard);
    else if (icon === 'home') void openHome();
  };

  // ---------- Rust events ----------
  const applyVisibility = (isHidden: boolean) => {
    live.current.hidden = isHidden;
    setHidden(isHidden);
    machine.setHidden(isHidden);
    if (isHidden) {
      live.current.surface = null;
      shownCard.current = null;
      pendingNotice.current = null;
      setSurface(null);
      setOut(false);
    }
  };

  /** A proactive card: only onto an idle, visible, awake pet; otherwise nothing is recorded. */
  const onWhatsNext = (e: EventPayloads['whats-next']) => {
    if (live.current.surface !== null || live.current.hidden || machine.output().body === 'sleep') return;
    machine.fire('wake');
    openNext(e.card, e.trigger, false);
  };

  const endHalo = () => {
    live.current.halo = 'fading';
    machine.set('dragOver', false);
    setHalo('fading');
    peekDelay.current = PEEK_AFTER_DRAG_MS;
    clear(haloTimer);
    haloTimer.current = window.setTimeout(() => {
      haloTimer.current = null;
      setHalo('none');
    }, HALO_FADE_MS);
  };

  const onRegion = (next: Region, dragging: boolean, target: string | null) => {
    const prev = live.current.region;
    live.current.region = next;
    setRegion(next);
    setHoverTarget(target);

    // Something is being dragged over the halo (but not our own drag-out).
    if (dragging && next === 'halo' && !dragOutActive.current) {
      clear(haloTimer);
      live.current.halo = 'on';
      setHalo('on');
      setOut(true);
      machine.set('dragOver', true);
    } else if (live.current.halo === 'on') {
      endHalo();
    }

    if (next === 'pet' && !dragging) {
      if (prev === null) {
        machine.fire('wake');
        setOut(true);
      }
      clear(restTimer);
      restTimer.current = window.setTimeout(() => {
        restTimer.current = null;
        if (live.current.surface === null) {
          live.current.surface = 'bubbles';
          setSurface('bubbles');
          machine.fire('gesture');
        }
      }, REST_MS);
    } else {
      clear(restTimer);
    }

    clear(foldTimer);
    if (next === null && live.current.surface === 'bubbles') {
      foldTimer.current = window.setTimeout(() => {
        foldTimer.current = null;
        if (live.current.surface === 'bubbles') closeSurface();
      }, FOLD_MS);
    }
  };

  const onHotkey = (action: HotkeyAction) => {
    if (action === 'focusPet') {
      machine.fire('wake');
      setOut(true);
      petRef.current?.focus();
    } else if (action === 'quickNote') {
      openSurface('quicknote');
    } else if (action === 'captureScreen') {
      closeSurface();
      runCapture(captureScreenshot);
    } else if (action === 'recordMeeting') {
      beginRecording();
    } else {
      runCapture(captureSelection);
    }
  };

  const onReaction = (e: EventPayloads['pet-reaction']) => {
    const c = e.category ? categories?.find((x) => x.id === e.category) : undefined;
    machine.fire(e.input, { icon: c && categoryIcon(c), color: c && categoryColor(c), variant: e.variant });
    if (e.input === 'idea' && e.category) {
      announce(t('pet.announce.savedTo', { category: categoryLabelOf(categories, e.category, t) }));
    }
    if (e.input === 'tilt') announce(t('pet.announce.savedTo', { category: categoryLabelOf(categories, 'unsorted', t) }));
  };

  // The subscriptions live for the window's lifetime and call the latest handlers.
  const handlers = useRef({ applyVisibility, onRegion, onHotkey, onReaction, onDropResult, onWhatsNext, onRecordingStatus });
  handlers.current = { applyVisibility, onRegion, onHotkey, onReaction, onDropResult, onWhatsNext, onRecordingStatus };

  useEffect(() => {
    const h = handlers;
    const subs = [
      on('pet-placement', setPlacement),
      on('pet-reaction', (e) => h.current.onReaction(e)),
      on('pet-flags', (f) => {
        if (f.thinking !== undefined) machine.set('thinking', f.thinking);
        if (f.carrying !== undefined) machine.set('carrying', f.carrying);
        if (f.sleep !== undefined) machine.set('sleep', f.sleep);
      }),
      on('pet-visibility', (v) => h.current.applyVisibility(v.hidden)),
      on('counts-changed', (c) => qc.setQueryData(COUNTS_KEY, c)),
      on('items-changed', () => void qc.invalidateQueries({ queryKey: TRAY_KEY })),
      on('pointer-region', (e) => h.current.onRegion(e.region, e.dragging, e.target)),
      on('hotkey', (e) => h.current.onHotkey(e.action)),
      on('capture-result', (r) => h.current.onDropResult(r)),
      on('whats-next', (e) => h.current.onWhatsNext(e)),
      on('recording-status', (s) => h.current.onRecordingStatus(s)),
      getCurrentWindow().onFocusChanged(({ payload: focused }) => {
        // A click outside (another app became key) closes the keyboard surfaces.
        if (!focused && KEYED.includes(live.current.surface)) setSurface(null);
      }),
    ];
    // Subscribe first, then read the current state so nothing in between is missed.
    void Promise.all(subs).then(() => {
      void getPlacement().then(setPlacement);
      void petStatus().then((s) => {
        h.current.applyVisibility(s.hidden);
        machine.set('sleep', s.sleep);
      });
    });
    return () => {
      for (const s of subs) void s.then((off) => off());
    };
  }, [machine, qc]);

  // Esc closes the notice and the bubbles too (the panels handle it themselves).
  useEffect(() => {
    if (surface !== 'notice' && surface !== 'bubbles') return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') closeSurface();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [surface, closeSurface]);

  // Back to peek once nothing holds the pet out.
  useEffect(() => {
    if (!out || region !== null || surface !== null || halo !== 'none' || petFocused || moving) return;
    const timer = window.setTimeout(() => {
      peekDelay.current = PEEK_MS;
      setOut(false);
    }, peekDelay.current);
    return () => window.clearTimeout(timer);
  }, [out, region, surface, halo, petFocused, moving]);

  // Badge changes are announced: the due-soon dot when it appears, else the unsorted count.
  const dueSoon = counts?.taskDueSoon ?? false;
  const lastBadge = useRef<{ dueSoon: boolean; unsorted: number } | null>(null);
  useEffect(() => {
    if (counts === undefined) return;
    const last = lastBadge.current;
    if (last && dueSoon && !last.dueSoon) {
      announce(t('pet.badge.dueSoon'));
    } else if (last && !dueSoon && last.unsorted !== unsorted && unsorted > 0) {
      announce(t('pet.badge.unsorted', { n: unsorted }));
    }
    lastBadge.current = { dueSoon, unsorted };
  }, [counts, dueSoon, unsorted, t]);

  // ---------- geometry ----------
  const size = applied?.size ?? 80;
  const area: Size | null = placement ? { w: placement.workArea.w, h: placement.workArea.h } : null;
  const edge = placement?.edge ?? 'right';
  const point = area && placement ? edgePoint(edge, placement.offset, area) : null;
  const asleep = output.body === 'sleep';
  const dockState =
    halo !== 'none' ? 'drag'
      : surface === 'bubbles' ? 'hover'
        : out || surface !== null ? 'out'
          : asleep ? 'sleep'
            : 'peek';
  const dockOut = dockState !== 'peek' && dockState !== 'sleep';

  const regionsKey = useRef('');
  useLayoutEffect(() => {
    if (!point || !area || moving) return;
    const regions: HitRegion[] = [];
    if (!hidden) {
      regions.push({ ...petHitRect(edge, point, size, dockOut), kind: 'pet' });
      if (surface === 'bubbles') {
        bubbleRects(edge, point, size, BUBBLES.length).forEach((r, i) => regions.push({ ...r, kind: 'panel', id: BUBBLES[i].icon }));
        // The gap between the pet and the arc keeps the bubbles out without taking clicks.
        regions.push({ ...bubbleZone(edge, point, size, BUBBLES.length), kind: 'zone' });
      }
      if (panelRect) regions.push({ ...panelRect, kind: 'panel' });
      if (hudRect) regions.push({ ...hudRect, kind: 'panel' });
    }
    const key = JSON.stringify(regions);
    if (key === regionsKey.current) return;
    regionsKey.current = key;
    void setHitRegions(regions);
  });

  // ---------- pet pointer: click, double-click, drag-move ----------
  const onPetPointerDown = (e: ReactPointerEvent<HTMLButtonElement>) => {
    if (e.button !== 0) return;
    const x0 = e.clientX;
    const y0 = e.clientY;
    let dragging = false;
    const onMove = (ev: PointerEvent) => {
      if (!dragging) {
        if (Math.hypot(ev.clientX - x0, ev.clientY - y0) <= MOVE_THRESHOLD) return;
        dragging = true;
        clear(restTimer);
        if (live.current.surface !== 'next') setSurface(null);
        void setPetMoving(true);
      }
      setMoving({ x: ev.clientX, y: ev.clientY });
    };
    const onUp = (ev: PointerEvent) => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onUp);
      if (!dragging) return;
      suppressClick.current = true;
      window.setTimeout(() => (suppressClick.current = false), 0);
      setMoving(null);
      // The window may have moved to another monitor meanwhile: use its current work area.
      const p = live.current.placement;
      if (!p) return;
      const snap = snapToEdge({ x: ev.clientX, y: ev.clientY }, { w: p.workArea.w, h: p.workArea.h }, size);
      setPlacement({ ...p, ...snap });
      void commitPetPosition(snap.edge, snap.offset).finally(() => void setPetMoving(false));
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onUp);
  };

  const onPetClick = () => {
    if (suppressClick.current) return;
    if (clickTimer.current !== null) {
      clear(clickTimer);
      goHome();
      return;
    }
    clickTimer.current = window.setTimeout(() => (clickTimer.current = null), DOUBLE_CLICK_MS);
    if (surface === 'tray') closeSurface();
    else openSurface('tray');
  };

  const openMenuAt = (p: Point) => {
    setMenuAt(p);
    openSurface('menu');
  };

  const onPetKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if ((e.key === 'F10' && e.shiftKey) || e.key === 'ContextMenu') {
      e.preventDefault();
      const r = petRef.current?.getBoundingClientRect();
      openMenuAt(r ? { x: r.left + r.width / 2, y: r.top + r.height / 2 } : { x: 0, y: 0 });
    } else if (e.key === 'Tab' && !e.shiftKey) {
      e.preventDefault();
      if (surface !== 'bubbles') openSurface('bubbles');
      // The arc mounts with the state change; focus its first bubble next frame.
      requestAnimationFrame(() => bubbleEls.current[0]?.focus());
    } else if (e.key === 'Escape' && surface !== null) {
      e.preventDefault();
      closeSurface();
    }
  };

  const onDockKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const at = bubbleEls.current.indexOf(e.target as HTMLButtonElement);
    if (at < 0) return;
    const n = BUBBLES.length;
    let next = -1;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (at + 1) % n;
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = (at - 1 + n) % n;
    else if (e.key === 'Escape') {
      e.preventDefault();
      closeSurface();
      petRef.current?.focus();
      return;
    }
    if (next >= 0) {
      e.preventDefault();
      bubbleEls.current[next]?.focus();
    }
  };

  const onDockFocus = (target: EventTarget) => {
    const i = bubbleEls.current.indexOf(target as HTMLButtonElement);
    setFocusIndex(i >= 0 ? i : undefined);
  };

  // WebKit's hover goes stale while the panel ignores the mouse, so the native hit test picks the bubble.
  const hoverIndex = surface === 'bubbles' ? BUBBLES.findIndex((b) => b.icon === hoverTarget) : -1;
  const hovered = hoverIndex >= 0 ? hoverIndex : undefined;

  // ---------- drop ----------
  // The native drop target in Rust takes the drop and captures it; the page only gets the result.
  function onDropResult(r: CaptureResult) {
    if (live.current.halo === 'on') endHalo();
    handleResult(r);
  }

  // ---------- render ----------
  const pose = poseFor(output.body, output.variant);
  const fx = fxForArt(output.fx);
  const art = applied ? (
    <CatArt
      look={applied.species}
      skin={applied.skin}
      parts={pose.parts}
      motion={paused ? null : pose.motion}
      motionMs={pose.ms}
      motionKey={output.seq}
      motionLoop={pose.loop}
      mirrored={edge === 'left'}
      reducedMotion={reducedMotion}
      edge={edge}
      fx={fx}
      hidden={hidden}
      recording={recState !== 'idle' ? { paused: recState === 'paused' } : undefined}
    />
  ) : null;

  // Floating re-places only when the resulting position changes, so these may change identity.
  const outBox = point ? petBox(edge, point, size) : null;
  const placeNextToPet = (s: Size) => (outBox && area ? placePanel(edge, outBox, s, area) : { x: 0, y: 0 });
  const placeMenu = (s: Size) => (menuAt && area ? placeAtPointer(edge, menuAt, s, area) : { x: 0, y: 0 });
  const closeNotice = useCallback(() => {
    if (live.current.surface === 'notice') closeSurface();
  }, [closeSurface]);

  if (!placement || !point || !area || !applied) return null;
  const box = dockBox(edge, point, area);

  return (
    <div className="pt-root">
      {moving ? (
        <div className="pt-moving" style={{ left: moving.x - size / 2, top: moving.y - size / 2, width: size, height: size }}>
          {art}
        </div>
      ) : null}
      <div
        className={`pt-dock pt-dock--${edge}${hidden ? ' is-hidden' : ''}${halo === 'fading' ? ' is-halo-out' : ''}${surface === 'bubbles' ? ' is-bubbles' : ''}`}
        style={{ left: box.x, top: box.y, width: DOCK_BOX, height: DOCK_BOX, '--pt-off': `${size + 8}px`, visibility: moving ? 'hidden' : undefined } as CSSProperties}
        aria-hidden={hidden || undefined}
        onKeyDown={onDockKeyDown}
        onFocus={(e) => onDockFocus(e.target)}
      >
        <PetDock
          ref={petRef}
          edge={edge}
          size={size}
          state={dockState}
          art={art}
          petName={name}
          bubbles={BUBBLES.map((b) => ({ icon: b.icon, label: t(b.label) }))}
          bubblesLabel={t('pet.actions')}
          focusIndex={hovered ?? focusIndex}
          activeIndex={hovered}
          onBubble={onBubble}
          bubbleFocusRef={(i, el) => {
            bubbleEls.current[i] = el;
          }}
          badge={
            hidden || asleep ? undefined
              : dueSoon ? { dot: true, label: t('pet.badge.dueSoon'), className: 'cp-pet__badge' }
                : unsorted > 0
                  ? { count: unsorted, label: t('pet.badge.unsorted', { n: unsorted }), className: 'cp-pet__badge' }
                  : undefined
          }
          petProps={{
            'aria-label': recState !== 'idle' ? `${t('pet.a11y', { name })}, ${t('rec.a11y')}` : t('pet.a11y', { name }),
            onClick: onPetClick,
            onPointerDown: onPetPointerDown,
            onContextMenu: (e) => {
              e.preventDefault();
              openMenuAt({ x: e.clientX, y: e.clientY });
            },
            onKeyDown: onPetKeyDown,
            onFocus: () => setPetFocused(true),
            onBlur: () => setPetFocused(false),
          }}
        />
      </div>

      {!hidden && surface === 'tray' ? (
        <Floating place={placeNextToPet} onRect={setPanelRect}>
          <TrayPanel
            areaHeight={area.h}
            petName={name}
            onClose={closeSurface}
            onPaste={paste}
            onOpenHome={() => goHome()}
            onOpenItem={(id) => goHome(`item/${encodeURIComponent(id)}`)}
            onDragStart={() => {
              dragOutActive.current = true;
              machine.fire('handOver');
            }}
            onDragEnd={(dropped) => {
              dragOutActive.current = false;
              if (dropped) closeSurface();
            }}
          />
        </Floating>
      ) : null}

      {!hidden && surface === 'menu' && menuAt ? (
        <Floating place={placeMenu} onRect={setPanelRect}>
          <PetMenu
            onClose={closeSurface}
            items={[
              { key: 'home', label: t('common.openHome'), hint: t('pet.menu.doubleClick'), onSelect: () => goHome() },
              {
                key: 'pause',
                label: paused ? t('pet.menu.resume') : t('pet.menu.pause'),
                onSelect: () => void setSetting('pet.paused', !paused),
              },
              {
                key: 'hide',
                label: t('pet.menu.hideHour'),
                onSelect: () => void setSetting('pet.hiddenUntil', Date.now() + HIDE_HOUR_MS),
              },
              null,
              { key: 'settings', label: t('common.settings'), onSelect: () => goHome('settings/general') },
              { key: 'quit', label: t('common.quit'), onSelect: () => void quitApp() },
            ]}
          />
        </Floating>
      ) : null}

      {!hidden && surface === 'quicknote' ? (
        <Floating place={placeNextToPet} onRect={setPanelRect}>
          <QuickNotePanel
            onClose={closeSurface}
            onSave={(text) => {
              closeSurface();
              runCapture(() => captureNote(text));
            }}
          />
        </Floating>
      ) : null}

      {!hidden && surface === 'next' && next ? (
        <Floating place={placeNextToPet} onRect={setPanelRect}>
          <NextPanel card={next.card} autoFocus={next.userInitiated} onAction={onNextAction} />
        </Floating>
      ) : null}

      {!hidden && surface === 'notice' && notice ? (
        <Floating place={placeNextToPet} onRect={setPanelRect}>
          <PetNotice
            notice={notice}
            onClose={closeNotice}
            onAction={() => {
              if (notice.permission) void requestPermission(notice.permission);
              if (notice.transcribeItemId) {
                const itemId = notice.transcribeItemId;
                closeSurface();
                transcribeItem(itemId).then(
                  (meetingId) => goHome(`meetings/${encodeURIComponent(meetingId)}`),
                  (e: unknown) => showNotice({ message: errorMessage(t, e) }),
                );
              } else if (notice.home !== undefined) goHome(notice.home);
              else closeSurface();
            }}
          />
        </Floating>
      ) : null}

      {!hidden && surface === 'consent' ? (
        <Floating place={placeNextToPet} onRect={setPanelRect}>
          <ConsentPanel onStart={onConsentStart} onClose={closeSurface} />
        </Floating>
      ) : null}

      {!hidden && surface === 'recLang' && menuAt ? (
        <Floating place={placeMenu} onRect={setPanelRect}>
          <PetMenu
            label={t('rec.hud.language')}
            onClose={closeSurface}
            items={REC_LANGUAGES.map(({ lang, label }) => ({
              key: lang,
              label: t(label),
              onSelect: () =>
                void setRecordingLanguage(lang).catch((e: unknown) => showNotice({ message: errorMessage(t, e) })),
            }))}
          />
        </Floating>
      ) : null}

      {!hidden && recordingData && recState !== 'idle' && WITH_HUD.includes(surface) ? (
        <Floating place={placeNextToPet} onRect={setHudRect} className="pt-floating--hud">
          <RecordingDock
            status={recordingData}
            receivedAt={recordingAt}
            onPause={onHudPause}
            onStop={onHudStop}
            onLanguage={onHudLanguage}
          />
        </Floating>
      ) : null}

      <div className="pt-sr" aria-live="polite">
        {announcement}
      </div>
    </div>
  );
}
