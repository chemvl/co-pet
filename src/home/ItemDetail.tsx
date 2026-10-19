// Item detail: the full page (`#/item/<id>`, Item.dc.html) and the compact pane beside the
// list (Home.dc.html). Sensitive items never render their payload.
import { useQuery } from '@tanstack/react-query';
import { convertFileSrc } from '@tauri-apps/api/core';
import { openPath, openUrl } from '@tauri-apps/plugin-opener';
import { useEffect, useRef, useState, type ReactElement, type ReactNode } from 'react';
import { Button, Icon, SegmentedControl, StatusPill, type ButtonProps } from '../ds';
import { categoryLabelOf } from '../shared/categories';
import { errorMessage, errorText } from '../shared/errors';
import { formatSize } from '../shared/format';
import { useLang, useT } from '../shared/i18n';
import { copyItems, transcribeItem } from '../shared/ipc';
import { useAddedBy, useAiStatus, useCategories, useSetting } from '../shared/queries';
import { useToast } from '../shared/Toast';
import type { ItemEdit, ItemDetail as Item } from '../shared/types';
import { CategoryTag } from './CategoryTag';
import { useDelayed } from './hooks';
import { KindTile, LocalPill } from './ItemRow';
import { detailMeta, extOf, historyText, hostOf, whenShort, whyText } from './itemText';
import { categoryMenuItems, Menu, MenuButton } from './Menu';
import { itemQuery } from './queries';
import { navigate } from './router';
import { useItemActions } from './useItemActions';
import { useSetSelection } from './selection';

const COPIED_MS = 1500;

interface ItemDetailProps {
  id: string;
  mode: 'page' | 'pane';
  onBack?: () => void;
}

export function ItemDetail({ id, mode, onBack }: ItemDetailProps): ReactElement {
  const t = useT();
  const q = useQuery(itemQuery(id));
  const slow = useDelayed(q.isPending, 400);
  if (q.data) {
    // Keyed by id so an open editor never carries over to another item.
    return mode === 'page' ? (
      <ItemPage key={q.data.id} item={q.data} onBack={onBack} />
    ) : (
      <ItemPane key={q.data.id} item={q.data} />
    );
  }
  if (q.isError) {
    return (
      <div className="hm-detailmsg">
        <p className="body">{t('item.notFound')}</p>
        {onBack ? (
          <Button variant="secondary" size="sm" onClick={onBack}>
            {t('common.back')}
          </Button>
        ) : null}
      </div>
    );
  }
  return slow ? <div className="hm-detailmsg hm-skeleton__block" aria-busy /> : <></>;
}

// ---------- page ----------

function ItemPage({ item, onBack }: { item: Item; onBack?: () => void }): ReactElement {
  const t = useT();
  const lang = useLang();
  useSetSelection(undefined, [item.id]);
  const editor = useItemEditor(item);
  return (
    <div className="hm-item">
      <div className="hm-item__bar">
        {onBack ? (
          <Button variant="quiet" size="sm" icon="undo" onClick={onBack}>
            {t('common.back')}
          </Button>
        ) : null}
        <div className="hm-item__head">
          {editor.editing ? <EditTitle editor={editor} /> : <div className="heading hm-ellipsis">{item.title}</div>}
          <div className="caption hm-muted hm-ellipsis">{detailMeta(item, t, lang)}</div>
          {item.fileAvailable ? null : <div className="caption hm-muted">{t('item.fileElsewhere')}</div>}
        </div>
        {editor.editing ? (
          <EditActions editor={editor} size="sm" />
        ) : (
          <>
            <ConvertButton item={item} />
            <TranscribeButton item={item} />
            <CopyButton item={item} />
            <OpenButton item={item} />
            <EditButton item={item} editor={editor} size="sm" variant="quiet" />
            <MoreMenu item={item} />
          </>
        )}
      </div>
      <div className="hm-item__body">
        <section className="hm-item__doc" aria-label={t('item.content')}>
          <ContentTabs item={item} editor={editor} />
          <Summary item={item} />
        </section>
        <aside className="hm-item__side" aria-label={t('item.properties')}>
          <Field label={t('item.category')}>
            <CategoryTrigger item={item} variant="select" />
          </Field>
          <Field label={t('item.why')}>
            <WhyBox item={item} variant="box" />
          </Field>
          <TaskBlock item={item} />
          {item.tags.length ? (
            <Field label={t('item.tags')}>
              <Tags item={item} />
            </Field>
          ) : null}
          <Field label={t('item.privacy')}>
            <PrivacyPills item={item} />
          </Field>
          <div className="hm-grow" />
          <Field label={t('item.history')}>
            <History item={item} />
          </Field>
        </aside>
      </div>
    </div>
  );
}

// ---------- pane ----------

function ItemPane({ item }: { item: Item }): ReactElement {
  const t = useT();
  const lang = useLang();
  const actions = useItemActions();
  const editor = useItemEditor(item);
  return (
    <div className="hm-det">
      <div className="hm-det__head">
        <KindTile item={item} large />
        <div className="hm-det__title">
          {editor.editing ? <EditTitle editor={editor} /> : <h2 className="heading hm-det__h">{item.title}</h2>}
          <div className="caption hm-muted">{detailMeta(item, t, lang)}</div>
          {item.fileAvailable ? null : <div className="caption hm-muted">{t('item.fileElsewhere')}</div>}
        </div>
        <Button
          variant="quiet"
          size="sm"
          icon="pin"
          className={item.pinned ? 'hm-is-on' : undefined}
          aria-label={t(item.pinned ? 'common.unpin' : 'common.pin')}
          aria-pressed={item.pinned}
          onClick={() => void actions.togglePin(item)}
        />
        <MoreMenu item={item} />
      </div>
      <div className="hm-det__chips">
        {item.triageStatus === 'pending' ? (
          <span className="caption hm-muted">{t('common.sorting')}</span>
        ) : (
          <CategoryTag id={item.categoryId} />
        )}
        {item.sensitive ? <StatusPill status="sensitive" label={t('common.sensitive')} /> : null}
        {item.addedBy ? <AgentPill clientId={item.addedBy} /> : null}
        <Tags item={item} />
      </div>
      <div className="hm-det__actions">
        {editor.editing ? (
          <EditActions editor={editor} size="md" />
        ) : (
          <>
            <ConvertButton item={item} size="md" />
            <TranscribeButton item={item} size="md" />
            <CopyButton item={item} size="md" />
            <OpenButton item={item} size="md" />
            <EditButton item={item} editor={editor} size="md" variant="secondary" />
          </>
        )}
      </div>
      <ContentTabs item={item} editor={editor} compact />
      <Summary item={item} />
      <WhyBox item={item} variant="line" />
      <div>
        <p className="hm-k">{t('item.history')}</p>
        <History item={item} />
      </div>
      <div className="hm-det__foot">
        <LocalPill item={item} localByDefault />
        <span className="caption hm-muted">{t('item.trayHint')}</span>
      </div>
    </div>
  );
}

// ---------- shared parts ----------

/** Meeting items follow their meeting; trashed items are read-only. */
function canEditTitle(item: Item): boolean {
  return item.meetingId == null && item.status !== 'trashed';
}

interface ItemEditor {
  editing: boolean;
  title: string;
  content: string;
  /** Whether this edit includes the content; decided when editing starts. */
  contentEditable: boolean;
  canSave: boolean;
  start(): void;
  cancel(): void;
  save(): Promise<void>;
  setTitle(title: string): void;
  setContent(content: string): void;
}

/** Inline title/content editing. Unsaved changes are dropped on Cancel or when the item changes. */
function useItemEditor(item: Item): ItemEditor {
  const actions = useItemActions();
  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState('');
  const [content, setContent] = useState('');
  const [contentEditable, setContentEditable] = useState(false);
  const [busy, setBusy] = useState(false);
  const canSave = !busy && title.trim() !== '' && (!contentEditable || content.trim() !== '');
  return {
    editing,
    title,
    content,
    contentEditable,
    canSave,
    start() {
      setTitle(item.title);
      setContent(item.payloadText ?? '');
      // Text kinds only; a sensitive item's content is hidden, so it can't be edited.
      const textKind = item.kind === 'text' || item.kind === 'richtext' || item.kind === 'code';
      setContentEditable(textKind && !item.sensitive);
      setEditing(true);
    },
    cancel() {
      setEditing(false);
    },
    async save() {
      if (!canSave) return;
      const edit: ItemEdit = {};
      if (title.trim() !== item.title) edit.title = title.trim();
      if (contentEditable && content !== (item.payloadText ?? '')) edit.content = content;
      if (edit.title === undefined && edit.content === undefined) {
        setEditing(false);
        return;
      }
      setBusy(true);
      const ok = await actions.edit(item, edit);
      setBusy(false);
      if (ok) setEditing(false);
    },
    setTitle,
    setContent,
  };
}

function EditTitle({ editor }: { editor: ItemEditor }): ReactElement {
  const t = useT();
  return (
    <input
      className="hm-edit-title heading"
      aria-label={t('item.editTitle')}
      autoFocus
      autoComplete="off"
      value={editor.title}
      onChange={(e) => editor.setTitle(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          void editor.save();
        } else if (e.key === 'Escape') {
          e.preventDefault();
          editor.cancel();
        }
      }}
    />
  );
}

function EditContent({ editor, kind, compact }: { editor: ItemEditor; kind: Item['kind']; compact?: boolean }): ReactElement {
  const t = useT();
  return (
    <textarea
      className={`hm-paper hm-edit-content ${kind === 'code' ? 'code' : 'body'}${compact ? ' hm-edit-content--compact' : ''}`}
      aria-label={t('item.content')}
      rows={compact ? 8 : 16}
      value={editor.content}
      onChange={(e) => editor.setContent(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          editor.cancel();
        } else if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
          e.preventDefault();
          void editor.save();
        }
      }}
    />
  );
}

function EditActions({ editor, size }: { editor: ItemEditor; size: ButtonProps['size'] }): ReactElement {
  const t = useT();
  return (
    <>
      <Button variant="quiet" size={size} onClick={editor.cancel}>
        {t('common.cancel')}
      </Button>
      <Button variant="primary" size={size} disabled={!editor.canSave} onClick={() => void editor.save()}>
        {t('common.save')}
      </Button>
    </>
  );
}

function EditButton({
  item,
  editor,
  size,
  variant,
}: {
  item: Item;
  editor: ItemEditor;
  size: ButtonProps['size'];
  variant: ButtonProps['variant'];
}): ReactElement | null {
  const t = useT();
  if (!canEditTitle(item)) return null;
  return (
    <Button variant={variant} size={size} onClick={editor.start}>
      {t('common.edit')}
    </Button>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }): ReactElement {
  return (
    <div>
      <p className="hm-k">{label}</p>
      {children}
    </div>
  );
}

/** Known error codes show their copy; anything else reads as a failed action. */
function useFail(): (err: unknown) => void {
  const t = useT();
  const { show } = useToast();
  return (err) => {
    const message = errorMessage(t, err);
    show({ message: message === errorText(err) ? t('home.actionFailed', { reason: message }) : message });
  };
}

function CopyButton({ item, size = 'sm' }: { item: Item; size?: ButtonProps['size'] }): ReactElement {
  const t = useT();
  const fail = useFail();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const id = window.setTimeout(() => setCopied(false), COPIED_MS);
    return () => window.clearTimeout(id);
  }, [copied]);
  return (
    <Button
      variant="secondary"
      size={size}
      icon={copied ? 'check' : 'copy'}
      onClick={() => copyItems([item.id]).then(() => setCopied(true), fail)}
    >
      {t(copied ? 'common.copied' : 'common.copy')}
    </Button>
  );
}

/** The surface's primary action: Open meeting for a meeting's item, else Convert to task
 *  (sensitive items convert too: the title is already visible). */
function ConvertButton({ item, size = 'sm' }: { item: Item; size?: ButtonProps['size'] }): ReactElement {
  const t = useT();
  const actions = useItemActions();
  const meetingId = item.meetingId;
  if (meetingId) {
    return (
      <Button variant="primary" size={size} icon="meeting" onClick={() => navigate(`meetings/${encodeURIComponent(meetingId)}`)}>
        {t('item.openMeeting')}
      </Button>
    );
  }
  return (
    <Button variant="primary" size={size} icon="tasks" onClick={() => void actions.convertToTask(item)}>
      {t('item.convertToTask')}
    </Button>
  );
}

/** Audio and video files become a meeting: transcribe, then open it. */
function TranscribeButton({ item, size = 'sm' }: { item: Item; size?: ButtonProps['size'] }): ReactElement | null {
  const t = useT();
  const fail = useFail();
  const [busy, setBusy] = useState(false);
  if (item.kind !== 'file' || !/^(audio|video)\//.test(item.mime ?? '')) return null;
  const run = () => {
    setBusy(true);
    transcribeItem(item.id)
      .then((id) => navigate(`meetings/${encodeURIComponent(id)}`), fail)
      .finally(() => setBusy(false));
  };
  return (
    <Button variant="secondary" size={size} icon="mic" disabled={busy} onClick={run}>
      {t('item.transcribe')}
    </Button>
  );
}

/** Side panel: the suggested task until one exists, then links to the item's tasks. */
function TaskBlock({ item }: { item: Item }): ReactElement | null {
  const t = useT();
  const actions = useItemActions();
  if (item.tasks.length) {
    return (
      <Field label={t('item.tasks')}>
        <ul className="hm-tasklinks">
          {item.tasks.map((task) => (
            <li key={task.id}>
              {task.status === 'done' ? <Icon name="check" size={16} label={t('tasks.done')} /> : null}
              <a className="hm-link" href={`#/tasks/${task.status}/${encodeURIComponent(task.id)}`}>
                {t('item.taskLink', { title: task.title })}
              </a>
            </li>
          ))}
        </ul>
      </Field>
    );
  }
  if (!item.taskSuggestion) return null;
  return (
    <Field label={t('item.suggestedTask')}>
      <div className="hm-tasksug">
        <span className="body">{item.taskSuggestion}</span>
        <Button variant="secondary" size="sm" onClick={() => void actions.convertToTask(item)}>
          {t('tasks.create')}
        </Button>
      </div>
    </Field>
  );
}

function OpenButton({ item, size = 'sm' }: { item: Item; size?: ButtonProps['size'] }): ReactElement | null {
  const t = useT();
  const fail = useFail();
  if (item.kind === 'url' && item.url) {
    const url = item.url;
    return (
      <Button variant="secondary" size={size} icon="link" onClick={() => openUrl(url).catch(fail)}>
        {t('common.openLink')}
      </Button>
    );
  }
  if (item.filePath) {
    const path = item.filePath;
    return (
      <Button
        variant="secondary"
        size={size}
        icon="export"
        disabled={!item.fileAvailable}
        onClick={() => openPath(path).catch(fail)}
      >
        {t('common.openFile')}
      </Button>
    );
  }
  return null;
}

function MoreMenu({ item }: { item: Item }): ReactElement {
  const t = useT();
  const actions = useItemActions();
  return (
    <MenuButton
      variant="quiet"
      size="sm"
      icon="more"
      align="end"
      aria-label={t('item.moreActions')}
      menuLabel={t('item.moreActions')}
      items={[
        {
          key: 'pin',
          label: t(item.pinned ? 'common.unpin' : 'common.pin'),
          onSelect: () => void actions.togglePin(item),
        },
        ...(item.status === 'inbox'
          ? [{ key: 'archive', label: t('common.archive'), onSelect: () => void actions.archive([item]) }]
          : []),
        ...(item.status !== 'trashed'
          ? [{ key: 'trash', label: t('common.moveToTrash'), danger: true, onSelect: () => void actions.trash([item]) }]
          : []),
      ]}
    />
  );
}

function SensitiveBox(): ReactElement {
  const t = useT();
  return (
    <div className="hm-paper hm-locked">
      <Icon name="lock" size={24} />
      <span className="body">{t('item.sensitiveHidden')}</span>
    </div>
  );
}

function ContentTabs({ item, editor, compact }: { item: Item; editor?: ItemEditor; compact?: boolean }): ReactElement {
  const t = useT();
  const [tab, setTab] = useState<'preview' | 'text'>('preview');
  if (editor?.editing && editor.contentEditable) {
    return (
      <div className={`hm-content${compact ? ' hm-content--compact' : ''}`}>
        <EditContent editor={editor} kind={item.kind} compact={compact} />
      </div>
    );
  }
  return (
    <div className={`hm-content${compact ? ' hm-content--compact' : ''}`}>
      <SegmentedControl
        label={t('item.view')}
        value={tab}
        onChange={(v) => setTab(v === 'text' ? 'text' : 'preview')}
        options={[
          { value: 'preview', label: t('item.preview') },
          { value: 'text', label: t('item.extractedText') },
        ]}
      />
      <div className="hm-content__view" role="tabpanel">
        {item.sensitive ? <SensitiveBox /> : tab === 'preview' ? <Preview item={item} /> : <ExtractedText item={item} />}
      </div>
    </div>
  );
}

function FileCard({ item }: { item: Item }): ReactElement {
  const lang = useLang();
  return (
    <div className="hm-paper hm-filecard">
      <KindTile item={{ ...item, thumbPath: null }} large />
      <div className="hm-filecard__text">
        <div className="body-strong hm-ellipsis">{item.fileName ?? item.title}</div>
        {item.sizeBytes != null ? <div className="caption hm-muted">{formatSize(item.sizeBytes, lang)}</div> : null}
      </div>
    </div>
  );
}

function Preview({ item }: { item: Item }): ReactElement {
  const src = item.filePath && item.fileAvailable ? convertFileSrc(item.filePath) : null;
  switch (item.kind) {
    case 'image':
    case 'screenshot':
      return src ? (
        <div className="hm-paper hm-media">
          <img src={src} alt={item.title} draggable={false} />
        </div>
      ) : (
        <FileCard item={item} />
      );
    case 'url': {
      const thumb = item.thumbPath ? convertFileSrc(item.thumbPath) : null;
      return (
        <div className="hm-paper hm-urlcard">
          {thumb ? <img className="hm-urlcard__img" src={thumb} alt="" draggable={false} /> : null}
          <div className="hm-urlcard__text">
            <div className="heading">{item.title}</div>
            {item.summary ? <p className="body">{item.summary}</p> : null}
            <div className="caption hm-muted hm-urlcard__host">
              <Icon name="link" size={14} />
              {hostOf(item.url) ?? item.url}
            </div>
          </div>
        </div>
      );
    }
    case 'text':
    case 'richtext':
      return <pre className="hm-paper hm-pre body">{item.payloadText ?? ''}</pre>;
    case 'code':
      return <pre className="hm-paper hm-pre code">{item.payloadText ?? ''}</pre>;
    case 'file':
      return src && (item.mime === 'application/pdf' || extOf(item.fileName) === 'pdf') ? (
        <iframe className="hm-paper hm-pdf" src={src} title={item.title} />
      ) : (
        <FileCard item={item} />
      );
  }
}

function ExtractedText({ item }: { item: Item }): ReactElement {
  const t = useT();
  if (item.extractedText) return <pre className="hm-paper hm-pre body">{item.extractedText}</pre>;
  return (
    <div className="hm-paper hm-empty-text body hm-muted">
      {item.extractStatus === 'pending' ? t('item.extracting') : t('item.noText')}
    </div>
  );
}

function Summary({ item }: { item: Item }): ReactElement | null {
  const t = useT();
  if (item.sensitive || !item.summary || item.kind === 'url') return null;
  return (
    <div>
      <p className="hm-k">{t('item.summary')}</p>
      <div className="body">{item.summary}</div>
    </div>
  );
}

/** Category select (page side panel) or "Change category" link (pane), both opening the category menu. */
function CategoryTrigger({ item, variant }: { item: Item; variant: 'select' | 'link' }): ReactElement {
  const t = useT();
  const actions = useItemActions();
  const { data: categories } = useCategories();
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button
        ref={anchor}
        type="button"
        className={variant === 'select' ? 'hm-select' : 'hm-link'}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={variant === 'select' ? `${t('item.category')}: ${categoryLabelOf(categories, item.categoryId, t)}` : undefined}
        onClick={() => setOpen((o) => !o)}
      >
        {variant === 'select' ? (
          <>
            <CategoryTag id={item.categoryId} />
            <span className="hm-grow" />
            <Icon name="chevron-down" size={16} />
          </>
        ) : (
          t('item.changeCategory')
        )}
      </button>
      {open ? (
        <Menu
          anchor={anchor}
          label={t('common.moveTo')}
          items={categoryMenuItems(t, categories ?? [], (cat) => void actions.moveTo([item], cat), item.categoryId)}
          onClose={(refocus) => {
            setOpen(false);
            if (refocus) anchor.current?.focus();
          }}
        />
      ) : null}
    </>
  );
}

function WhyBox({ item, variant }: { item: Item; variant: 'box' | 'line' }): ReactElement | null {
  const t = useT();
  const lang = useLang();
  const { data: ai } = useAiStatus();
  const activeModel = useSetting<string | null>('ai.modelId', null);
  const modelId = activeModel ?? ai?.modelId ?? ai?.recommendedId;
  const bytes = ai?.models.find((m) => m.id === modelId)?.bytes ?? ai?.bytesTotal ?? 0;
  const why = whyText(item, t, bytes ? formatSize(bytes, lang) : '');
  if (!why) return variant === 'line' ? null : <div className="hm-why hm-why--box caption hm-muted">—</div>;
  const pct = why.confidence != null ? Math.round(why.confidence * 100) : null;
  const confident = item.triageStatus === 'done' && item.categoryId !== 'unsorted';
  return (
    <div className={`hm-why hm-why--${variant}`}>
      {variant === 'line' ? (
        <span className={`hm-why__icon${confident ? ' is-ok' : ''}`}>
          <Icon name={item.triageStatus === 'pending' ? 'clock' : confident ? 'check' : 'unsorted'} size={18} />
        </span>
      ) : null}
      <div className="hm-why__text">
        <span>{why.text}</span>
        {variant === 'line' ? (
          <>
            {' '}
            <CategoryTrigger item={item} variant="link" />
          </>
        ) : null}
        {pct != null ? (
          <>
            <div className="hm-meter" role="img" aria-label={t('item.why.confidenceA11y', { n: pct })}>
              <i style={{ width: `${pct}%` }} />
            </div>
            <span className="caption hm-muted">{why.provider ? t('item.why.confidenceCloud', { n: pct, provider: why.provider }) : t('item.why.confidence', { n: pct })}</span>
          </>
        ) : null}
      </div>
    </div>
  );
}

function Tags({ item }: { item: Item }): ReactElement | null {
  if (!item.tags.length) return null;
  return (
    <span className="hm-tags">
      {item.tags.map((tag) => (
        <span key={tag} className="hm-tag">
          {tag}
        </span>
      ))}
    </span>
  );
}

function PrivacyPills({ item }: { item: Item }): ReactElement {
  const t = useT();
  return (
    <div className="hm-tags">
      <LocalPill item={item} localByDefault />
      {item.sensitive ? <StatusPill status="sensitive" label={t('common.sensitive')} /> : null}
      {item.addedBy ? <AgentPill clientId={item.addedBy} /> : null}
    </div>
  );
}

function AgentPill({ clientId }: { clientId: string }): ReactElement {
  const addedBy = useAddedBy();
  return <StatusPill status="agent" label={addedBy(clientId)} />;
}

function History({ item }: { item: Item }): ReactElement {
  const t = useT();
  const lang = useLang();
  const actions = useItemActions();
  const { data: categories } = useCategories();
  // Events arrive newest first; only the latest live user change can be undone here.
  const undoable = item.events.find((e) => e.actor === 'user' && e.action !== 'captured' && e.undoneAt == null);
  return (
    <ul className="hm-hist">
      {item.events.map((ev) => (
        <li key={ev.id} className={`hm-hist__row${ev.undoneAt != null ? ' is-undone' : ''}`}>
          <span className="hm-grow">
            {historyText(ev, item, t, categories)} · {whenShort(ev.at, lang, true)}
          </span>
          {ev.undoneAt != null ? (
            <span className="caption hm-muted">{t('item.history.undone')}</span>
          ) : ev.id === undoable?.id ? (
            <button type="button" className="hm-link" onClick={() => void actions.undo([ev.id])}>
              {t('common.undo')}
            </button>
          ) : null}
        </li>
      ))}
    </ul>
  );
}
