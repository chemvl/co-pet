// Task editor beside the Tasks list (over it below 1100 px). Create mode has a "Create task"
// button; an existing task saves each field 400 ms after the last change. Esc closes.
import { useQuery } from '@tanstack/react-query';
import { useCallback, useEffect, useId, useRef, useState, type FormEvent, type ReactElement, type ReactNode } from 'react';
import { Button, Icon, SegmentedControl } from '../../ds';
import { errorMessage, errorText } from '../../shared/errors';
import { formatEstimate } from '../../shared/format';
import { useLang, useT } from '../../shared/i18n';
import { updateTask } from '../../shared/ipc';
import type { NewTask, TaskDetail, TaskPatch, TaskSummary } from '../../shared/types';
import { useDelayed } from '../hooks';
import { historyLabel, undoableEvent } from './history';
import { whenShort } from '../itemText';
import { taskQuery } from '../queries';
import { useTaskActions } from './useTaskActions';

const SAVE_DEBOUNCE_MS = 400;
const SKELETON_DELAY_MS = 400;
const ESTIMATES = [5, 10, 15, 30, 45, 60, 120, 240];
const PRIORITIES = ['none', 'low', 'medium', 'high'] as const;

/** The editable fields as the form holds them: '' for an empty date, time or first step. */
interface Draft {
  title: string;
  notes: string;
  dueDate: string;
  dueTime: string;
  estimateMin: number | null;
  priority: number;
  firstStep: string;
}

const EMPTY_DRAFT: Draft = { title: '', notes: '', dueDate: '', dueTime: '', estimateMin: null, priority: 0, firstStep: '' };
const DRAFT_KEYS = Object.keys(EMPTY_DRAFT) as (keyof Draft)[];

function draftOf(task: TaskDetail): Draft {
  return {
    title: task.title,
    notes: task.notes,
    dueDate: task.dueDate ?? '',
    dueTime: task.dueTime ?? '',
    estimateMin: task.estimateMin,
    priority: task.priority,
    firstStep: task.firstStep ?? '',
  };
}

/** The patch for changed fields, as the store keeps them. An empty title is left out (it's required). */
function patchOf(c: Partial<Draft>): TaskPatch {
  const patch: TaskPatch = {};
  if (c.title?.trim()) patch.title = c.title.trim();
  if (c.notes !== undefined) patch.notes = c.notes;
  if (c.dueDate !== undefined) patch.dueDate = c.dueDate || null;
  if (c.dueTime !== undefined) patch.dueTime = c.dueTime || null;
  if (c.estimateMin !== undefined) patch.estimateMin = c.estimateMin;
  if (c.priority !== undefined) patch.priority = c.priority;
  if (c.firstStep !== undefined) patch.firstStep = c.firstStep.trim() ? c.firstStep : null;
  return patch;
}

interface TaskEditorProps {
  /** Absent: create mode. */
  id?: string;
  /** Below 1100 px the editor covers the list and shows Back instead of Close. */
  overlay: boolean;
  onClose: () => void;
  onCreated: (task: TaskSummary) => void;
}

export function TaskEditor({ id, overlay, onClose, onCreated }: TaskEditorProps): ReactElement {
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return;
      e.preventDefault();
      closeRef.current();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  return id ? (
    <ExistingTask id={id} overlay={overlay} onClose={onClose} />
  ) : (
    <CreateTask overlay={overlay} onClose={onClose} onCreated={onCreated} />
  );
}

// ---------- create ----------

function CreateTask(p: { overlay: boolean; onClose: () => void; onCreated: (task: TaskSummary) => void }): ReactElement {
  const t = useT();
  const actions = useTaskActions();
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const title = draft.title.trim();
    if (!title || busy) return;
    const input: NewTask = { title, priority: draft.priority };
    if (draft.notes) input.notes = draft.notes;
    if (draft.dueDate) input.dueDate = draft.dueDate;
    if (draft.dueDate && draft.dueTime) input.dueTime = draft.dueTime;
    if (draft.estimateMin != null) input.estimateMin = draft.estimateMin;
    if (draft.firstStep.trim()) input.firstStep = draft.firstStep;
    setBusy(true);
    const created = await actions.create(input);
    setBusy(false);
    if (created) p.onCreated(created);
  };

  return (
    <form className={`tk-editor${p.overlay ? ' tk-editor--overlay' : ''}`} aria-label={t('tasks.new')} onSubmit={(e) => void submit(e)}>
      <EditorBar overlay={p.overlay} onClose={p.onClose} title={t('tasks.new')} />
      <Fields draft={draft} onChange={(c) => setDraft((d) => ({ ...d, ...c }))} autoFocus />
      <Field label={t('tasks.field.source')}>
        <span className="caption hm-muted">{t('tasks.noSource')}</span>
      </Field>
      <div className="tk-editor__foot">
        <Button type="submit" variant="primary" disabled={!draft.title.trim() || busy}>
          {t('tasks.create')}
        </Button>
      </div>
    </form>
  );
}

// ---------- existing ----------

function ExistingTask({ id, overlay, onClose }: { id: string; overlay: boolean; onClose: () => void }): ReactElement {
  const t = useT();
  // A task that is gone (trashed here or elsewhere, its creation undone) closes the editor.
  const q = useQuery({ ...taskQuery(id), retry: (n, err) => errorText(err) !== 'notFound' && n < 1 });
  const gone = q.isError && errorText(q.error) === 'notFound';
  const slow = useDelayed(q.isPending, SKELETON_DELAY_MS);
  useEffect(() => {
    if (gone) onClose();
  }, [gone, onClose]);

  if (q.data && !gone) return <EditForm key={q.data.id} task={q.data} overlay={overlay} onClose={onClose} />;
  return (
    <div className={`tk-editor${overlay ? ' tk-editor--overlay' : ''}`} aria-busy={q.isPending}>
      <EditorBar overlay={overlay} onClose={onClose} />
      {q.isError ? (
        <p className="body">{errorMessage(t, q.error)}</p>
      ) : slow ? (
        <div className="hm-skeleton__block" />
      ) : null}
    </div>
  );
}

function EditForm({ task, overlay, onClose }: { task: TaskDetail; overlay: boolean; onClose: () => void }): ReactElement {
  const t = useT();
  const actions = useTaskActions();
  const [draft, setDraft] = useState<Draft>(() => draftOf(task));
  // `base`: the stored values the form last agreed with. Server updates overwrite only the
  // fields that moved away from it, so a save's own echo never clobbers typing.
  const base = useRef<Draft>(draftOf(task));
  const pending = useRef<Partial<Draft>>({});
  const inFlight = useRef(0);
  const timer = useRef<number | undefined>(undefined);
  const { fail } = actions;

  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    timer.current = undefined;
    const changes = pending.current;
    pending.current = {};
    const patch = patchOf(changes);
    if (!Object.keys(patch).length) return;
    const saved = { ...changes };
    if (patch.title) saved.title = patch.title;
    else delete saved.title;
    base.current = { ...base.current, ...saved };
    inFlight.current += 1;
    try {
      await updateTask(task.id, patch);
    } catch (err) {
      fail(err);
    } finally {
      inFlight.current -= 1;
    }
  }, [task.id, fail]);

  // Unmount (close, another task) saves what is still waiting.
  useEffect(() => () => void flush(), [flush]);

  useEffect(() => {
    if (Object.keys(pending.current).length || inFlight.current) return;
    const server = draftOf(task);
    setDraft((d) => {
      const next = { ...d };
      for (const k of DRAFT_KEYS) if (server[k] !== base.current[k]) Object.assign(next, { [k]: server[k] });
      return next;
    });
    base.current = server;
  }, [task]);

  const change = (c: Partial<Draft>) => {
    setDraft((d) => ({ ...d, ...c }));
    pending.current = { ...pending.current, ...c };
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void flush(), SAVE_DEBOUNCE_MS);
  };

  const moveToTrash = async () => {
    await flush();
    if (await actions.trash(task)) onClose();
  };

  return (
    <section className={`tk-editor${overlay ? ' tk-editor--overlay' : ''}`} aria-label={t('tasks.selectedTask')}>
      <EditorBar overlay={overlay} onClose={onClose} />
      <Fields draft={draft} onChange={change} requireTitle />
      <Field label={t('tasks.field.source')}>
        {task.sourceItemId && task.sourceTitle ? (
          <a
            className="hm-link tk-source"
            href={
              task.sourceMeetingId
                ? `#/meetings/${encodeURIComponent(task.sourceMeetingId)}`
                : `#/item/${encodeURIComponent(task.sourceItemId)}`
            }
          >
            <Icon name={task.sourceMeetingId ? 'meeting' : 'inbox'} size={16} />
            <span className="hm-ellipsis">{task.sourceTitle}</span>
          </a>
        ) : (
          <span className="caption hm-muted">{t('tasks.noSource')}</span>
        )}
      </Field>
      <Field label={t('item.history')}>
        <History task={task} onUndo={(eventId) => void actions.undo([eventId])} />
      </Field>
      <div className="tk-editor__foot">
        <Button variant="quiet" size="sm" icon="trash" onClick={() => void moveToTrash()}>
          {t('common.moveToTrash')}
        </Button>
      </div>
    </section>
  );
}

// ---------- parts ----------

function EditorBar({ overlay, onClose, title }: { overlay: boolean; onClose: () => void; title?: string }): ReactElement {
  const t = useT();
  return (
    <div className="tk-editor__bar">
      {overlay ? (
        <Button variant="quiet" size="sm" icon="undo" onClick={onClose}>
          {t('common.back')}
        </Button>
      ) : null}
      <h2 className="heading tk-editor__h">{title}</h2>
      {overlay ? null : <Button variant="quiet" size="sm" icon="close" aria-label={t('common.close')} onClick={onClose} />}
    </div>
  );
}

function Field({ label, htmlFor, children }: { label: string; htmlFor?: string; children: ReactNode }): ReactElement {
  return (
    <div className="tk-field">
      {htmlFor ? (
        <label className="hm-k" htmlFor={htmlFor}>{label}</label>
      ) : (
        <p className="hm-k">{label}</p>
      )}
      {children}
    </div>
  );
}

function Fields(p: { draft: Draft; onChange: (c: Partial<Draft>) => void; autoFocus?: boolean; requireTitle?: boolean }): ReactElement {
  const { draft, onChange } = p;
  const t = useT();
  const lang = useLang();
  const ids = useId();
  const titleMissing = !!p.requireTitle && !draft.title.trim();
  // An estimate set elsewhere (agent, What's Next) stays selectable.
  const estimates =
    draft.estimateMin == null || ESTIMATES.includes(draft.estimateMin)
      ? ESTIMATES
      : [...ESTIMATES, draft.estimateMin].sort((a, b) => a - b);

  return (
    <>
      <Field label={t('tasks.field.title')} htmlFor={`${ids}-title`}>
        <input
          id={`${ids}-title`}
          className="tk-input"
          required
          maxLength={300}
          autoFocus={p.autoFocus}
          aria-invalid={titleMissing || undefined}
          aria-describedby={titleMissing ? `${ids}-title-err` : undefined}
          value={draft.title}
          onChange={(e) => onChange({ title: e.currentTarget.value })}
        />
        {titleMissing ? (
          <p className="tk-error" id={`${ids}-title-err`}>{t('tasks.field.titleRequired')}</p>
        ) : null}
      </Field>
      <Field label={t('tasks.field.notes')} htmlFor={`${ids}-notes`}>
        <textarea
          id={`${ids}-notes`}
          className="tk-input tk-textarea"
          rows={4}
          value={draft.notes}
          onChange={(e) => onChange({ notes: e.currentTarget.value })}
        />
      </Field>
      <Field label={t('tasks.field.due')}>
        <div className="tk-due">
          <input
            type="date"
            className="tk-input tk-input--date"
            aria-label={t('tasks.field.dueDate')}
            value={draft.dueDate}
            onChange={(e) => {
              const v = e.currentTarget.value;
              onChange(v ? { dueDate: v } : { dueDate: '', dueTime: '' });
            }}
          />
          <input
            type="time"
            className="tk-input tk-input--time"
            aria-label={t('tasks.field.dueTime')}
            disabled={!draft.dueDate}
            value={draft.dueTime}
            onChange={(e) => onChange({ dueTime: e.currentTarget.value })}
          />
          <Button
            variant="quiet"
            size="sm"
            disabled={!draft.dueDate && !draft.dueTime}
            onClick={() => onChange({ dueDate: '', dueTime: '' })}
          >
            {t('tasks.field.clearDue')}
          </Button>
        </div>
      </Field>
      <Field label={t('tasks.field.estimate')} htmlFor={`${ids}-estimate`}>
        <select
          id={`${ids}-estimate`}
          className="tk-input"
          value={draft.estimateMin ?? ''}
          onChange={(e) => onChange({ estimateMin: e.currentTarget.value ? Number(e.currentTarget.value) : null })}
        >
          <option value="">{t('tasks.estimate.none')}</option>
          {estimates.map((m) => (
            <option key={m} value={m}>
              {formatEstimate(m, lang)}
            </option>
          ))}
        </select>
      </Field>
      <Field label={t('tasks.priority')}>
        <SegmentedControl
          label={t('tasks.priority')}
          value={String(draft.priority)}
          options={PRIORITIES.map((name, i) => ({ value: String(i), label: t(`tasks.priority.${name}`) }))}
          onChange={(v) => onChange({ priority: Number(v) })}
        />
      </Field>
      <Field label={t('tasks.firstStep')} htmlFor={`${ids}-step`}>
        <input
          id={`${ids}-step`}
          className="tk-input"
          value={draft.firstStep}
          onChange={(e) => onChange({ firstStep: e.currentTarget.value })}
        />
      </Field>
    </>
  );
}

function History({ task, onUndo }: { task: TaskDetail; onUndo: (eventId: string) => void }): ReactElement {
  const t = useT();
  const lang = useLang();
  const undoable = undoableEvent(task.events);
  return (
    <ul className="hm-hist">
      {task.events.map((ev) => (
        <li key={ev.id} className={`hm-hist__row${ev.undoneAt != null ? ' is-undone' : ''}`}>
          <span className="hm-grow">
            {historyLabel(ev, t)} ·{' '}
            {whenShort(ev.at, lang, true)}
          </span>
          {ev.undoneAt != null ? (
            <span className="caption hm-muted">{t('item.history.undone')}</span>
          ) : ev.id === undoable?.id ? (
            <button type="button" className="hm-link" onClick={() => onUndo(ev.id)}>
              {t('common.undo')}
            </button>
          ) : null}
        </li>
      ))}
    </ul>
  );
}
