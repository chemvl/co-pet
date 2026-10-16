// Home › Categories (screen 13, CatRules.dc.html): the live categories in sidebar order (drag
// or Alt+↑/↓ to reorder; Unsorted stays last), and an editor for the selected one: name, icon,
// colour, rules, learned examples, the AI/agents switches and Delete category.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useId, useState, type KeyboardEvent, type ReactElement } from 'react';
import { Button, CategoryChip, Icon, ICON_NAMES, Switch, type IconName } from '../../ds';
import { categoryColor, categoryIcon, categoryLabel } from '../../shared/categories';
import { errorMessage, errorText } from '../../shared/errors';
import { formatDate } from '../../shared/format';
import { useLang, useT, type I18nKey } from '../../shared/i18n';
import {
  addRule, createCategory, deleteCategory, deleteExample, deleteRule, listExamples, listRules, makeRuleFromItem, on,
  reorderCategories, restoreRule, updateCategory,
} from '../../shared/ipc';
import { CATEGORIES_KEY, useCategories } from '../../shared/queries';
import { useToast } from '../../shared/Toast';
import {
  BUILTIN_CATEGORY_IDS, type Category, type CategoryPatch, type ItemKind, type Rule, type SourceKind,
} from '../../shared/types';
import { countsQuery } from '../queries';
import { navigate } from '../router';
import { Dialog } from '../settings/parts';
import { ruleRows, ruleText } from './RuleSentence';
import './categories.css';

/** Every icon except UI-chrome glyphs. */
const CHROME_ICONS: readonly string[] = ['more', 'plus', 'trash', 'undo', 'check', 'grip', 'close'];
const PICKER_ICONS = ICON_NAMES.filter((n) => !n.startsWith('chevron-') && !CHROME_ICONS.includes(n));
/** The built-in colour pairs a category can use (Unsorted's is reserved). */
const SWATCHES = BUILTIN_CATEGORY_IDS.filter((id) => id !== 'unsorted');

const RULE_KINDS: readonly Rule['kind'][] = ['domain', 'extension', 'item_kind', 'source_kind', 'source_app', 'regex'];
const ITEM_KINDS: readonly ItemKind[] = ['file', 'image', 'url', 'text', 'richtext', 'code', 'screenshot'];
const SOURCE_KINDS: readonly SourceKind[] = ['drop', 'clipboard', 'screenshot', 'note', 'selection'];

export function CategoriesPage({ selectedId }: { selectedId?: string }): ReactElement {
  const qc = useQueryClient();
  const { data: categories } = useCategories();
  const [creating, setCreating] = useState(false);
  const selected = categories?.find((c) => c.id === selectedId) ?? categories?.[0];

  // Rule match counts follow item moves; rules and examples follow category writes.
  useEffect(() => {
    const refresh = () => {
      void qc.invalidateQueries({ queryKey: ['rules'] });
      void qc.invalidateQueries({ queryKey: ['examples'] });
    };
    const subs = [on('categories-changed', refresh), on('items-changed', refresh)];
    return () => subs.forEach((s) => void s.then((unlisten) => unlisten()));
  }, [qc]);

  return (
    <div className="cg-page">
      <CategoryList categories={categories ?? []} selectedId={selected?.id} onNew={() => setCreating(true)} />
      {selected ? <Editor key={selected.id} category={selected} /> : <div className="cg-editor" />}
      {creating ? <NewCategoryDialog onClose={() => setCreating(false)} /> : null}
    </div>
  );
}

function CategoryList(p: { categories: Category[]; selectedId?: string; onNew: () => void }): ReactElement {
  const t = useT();
  const qc = useQueryClient();
  const { show } = useToast();
  const { data: counts } = useQuery(countsQuery);
  const [dragId, setDragId] = useState<string | null>(null);
  const headId = useId();

  const reorder = (id: string, to: number) => {
    const order = p.categories.filter((c) => c.id !== id);
    order.splice(to, 0, p.categories.find((c) => c.id === id)!);
    // Unsorted stays last whatever the drop target.
    const ids = [...order.filter((c) => c.id !== 'unsorted'), ...order.filter((c) => c.id === 'unsorted')];
    qc.setQueryData<Category[]>(CATEGORIES_KEY, ids.map((c, position) => ({ ...c, position })));
    reorderCategories(ids.map((c) => c.id)).catch((e) => {
      show({ message: errorMessage(t, e) });
      void qc.invalidateQueries({ queryKey: CATEGORIES_KEY });
    });
  };

  const onKeyDown = (e: KeyboardEvent, c: Category, index: number) => {
    if (!e.altKey || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') || c.id === 'unsorted') return;
    e.preventDefault();
    const to = index + (e.key === 'ArrowUp' ? -1 : 1);
    if (to < 0 || p.categories[to]?.id === 'unsorted' || to >= p.categories.length) return;
    reorder(c.id, to);
  };

  return (
    <nav className="cg-list" aria-labelledby={headId}>
      <h1 className="cg-list__title" id={headId}>{t('home.categories')}</h1>
      <ul className="cg-rows">
        {p.categories.map((c, index) => {
          const pinned = c.id === 'unsorted';
          const n = pinned ? counts?.unsorted : counts?.byCategory[c.id];
          return (
            <li
              key={c.id}
              className={`cg-row${c.id === p.selectedId ? ' is-sel' : ''}${dragId === c.id ? ' is-drag' : ''}`}
              draggable={!pinned}
              onDragStart={(e) => {
                setDragId(c.id);
                e.dataTransfer.effectAllowed = 'move';
                e.dataTransfer.setData('text/plain', c.id);
              }}
              onDragEnd={() => setDragId(null)}
              onDragOver={(e) => {
                if (dragId && !pinned) e.preventDefault();
              }}
              onDrop={(e) => {
                e.preventDefault();
                if (dragId && dragId !== c.id && !pinned) reorder(dragId, index);
                setDragId(null);
              }}
            >
              <a
                className="cg-row__link"
                href={`#/categories/${encodeURIComponent(c.id)}`}
                draggable={false}
                aria-current={c.id === p.selectedId ? 'page' : undefined}
                aria-keyshortcuts={pinned ? undefined : 'Alt+ArrowUp Alt+ArrowDown'}
                onKeyDown={(e) => onKeyDown(e, c, index)}
              >
                <span className={`cg-grip${pinned ? ' is-hidden' : ''}`} aria-hidden="true">
                  <Icon name="grip" size={16} />
                </span>
                <CategoryChip category={categoryColor(c)} icon={categoryIcon(c)} label={categoryLabel(c, t)} size="sm" />
                {c.excludeFromAi ? (
                  <span className="cg-noai caption">
                    <Icon name="lock" size={14} />
                    {t('categories.notSentToAi')}
                  </span>
                ) : null}
                <span className="cg-row__count">{n ?? ''}</span>
              </a>
            </li>
          );
        })}
      </ul>
      <Button variant="secondary" size="sm" icon="plus" className="cg-new" onClick={p.onNew}>
        {t('categories.new')}
      </Button>
      <div className="cg-grow" />
      <span className="caption cg-muted cg-list__cap">{t('categories.caption')}</span>
    </nav>
  );
}

function IconPicker(p: { value: IconName; onChange: (icon: IconName) => void; labelledBy: string }): ReactElement {
  const t = useT();
  const name = useId();
  return (
    <div className="cg-grid" role="radiogroup" aria-labelledby={p.labelledBy}>
      {PICKER_ICONS.map((icon) => (
        <label key={icon} className="cg-ib">
          <input
            type="radio"
            className="cg-sr"
            name={name}
            checked={p.value === icon}
            aria-label={t(`icon.${icon}` as I18nKey)}
            onChange={() => p.onChange(icon)}
          />
          <Icon name={icon} size={18} />
        </label>
      ))}
    </div>
  );
}

function ColourPicker(p: { value: string; onChange: (color: string) => void; labelledBy: string }): ReactElement {
  const t = useT();
  const name = useId();
  return (
    <div className="cg-grid cg-grid--swatches" role="radiogroup" aria-labelledby={p.labelledBy}>
      {SWATCHES.map((id) => (
        <label key={id} className="cg-swatch" style={{ background: `var(--cat-${id})` }}>
          <input
            type="radio"
            className="cg-sr"
            name={name}
            checked={p.value === id}
            aria-label={t(`category.${id}`)}
            onChange={() => p.onChange(id)}
          />
        </label>
      ))}
    </div>
  );
}

function Editor({ category: c }: { category: Category }): ReactElement {
  const t = useT();
  const { show } = useToast();
  const { data: counts } = useQuery(countsQuery);
  const label = categoryLabel(c, t);
  const unsorted = c.id === 'unsorted';
  const count = (unsorted ? counts?.unsorted : counts?.byCategory[c.id]) ?? 0;
  const nameId = useId();
  const iconId = useId();
  const colourId = useId();
  const [name, setName] = useState(label);
  const [nameError, setNameError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  const update = (patch: CategoryPatch) =>
    updateCategory(c.id, patch).catch((e) => show({ message: errorMessage(t, e) }));

  const saveName = async () => {
    const v = name.trim();
    if (v === label) {
      setName(label);
      setNameError(null);
      return;
    }
    try {
      await updateCategory(c.id, { name: v });
      setName(v);
      setNameError(null);
    } catch (e) {
      setNameError(errorMessage(t, e));
    }
  };

  return (
    <section className="cg-editor" aria-label={t('categories.editName', { name: label })}>
      <div className="cg-two">
        <div>
          <label className="cg-k" htmlFor={nameId}>{t('categories.name')}</label>
          <input
            id={nameId}
            className="st-input cg-input"
            value={name}
            autoComplete="off"
            aria-invalid={nameError ? true : undefined}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => void saveName()}
            onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
          />
          {nameError ? <p className="st-error" role="alert">{nameError}</p> : null}
        </div>
        <div>
          <span className="cg-k">{t('categories.items')}</span>
          <div className="cg-items">
            {t('categories.itemCount', { n: count })} ·{' '}
            <a className="hm-link" href={`#/category/${encodeURIComponent(c.id)}`}>{t('categories.showInInbox')}</a>
          </div>
        </div>
      </div>
      {unsorted ? null : (
        <div className="cg-two">
          <div>
            <span className="cg-k" id={iconId}>{t('categories.icon')}</span>
            <IconPicker value={categoryIcon(c)} labelledBy={iconId} onChange={(icon) => void update({ icon })} />
          </div>
          <div>
            <span className="cg-k" id={colourId}>{t('categories.colour')}</span>
            <ColourPicker value={categoryColor(c)} labelledBy={colourId} onChange={(color) => void update({ color })} />
          </div>
        </div>
      )}
      <Rules category={c} />
      <Examples category={c} />
      {unsorted ? null : (
        <>
          <div className="cg-two">
            <Switch
              label={t('categories.noAi')}
              description={t('categories.noAiDesc')}
              checked={c.excludeFromAi}
              onChange={(v) => void update({ excludeFromAi: v })}
            />
            <Switch
              label={t('categories.hideAgents')}
              description={t('categories.hideAgentsDesc', { name: label })}
              checked={c.excludeFromMcp}
              onChange={(v) => void update({ excludeFromMcp: v })}
            />
          </div>
          <div>
            <Button variant="quiet" size="sm" icon="trash" onClick={() => setDeleting(true)}>
              {t('categories.delete')}
            </Button>
          </div>
        </>
      )}
      {deleting ? <DeleteDialog category={c} label={label} count={count} onClose={() => setDeleting(false)} /> : null}
    </section>
  );
}

function Rules({ category: c }: { category: Category }): ReactElement {
  const t = useT();
  const qc = useQueryClient();
  const { show } = useToast();
  const headId = useId();
  const { data: rules } = useQuery({ queryKey: ['rules', c.id], queryFn: () => listRules(c.id) });
  const [adding, setAdding] = useState(false);
  const refresh = () => void qc.invalidateQueries({ queryKey: ['rules', c.id] });

  const remove = async (ids: string[]) => {
    try {
      await Promise.all(ids.map(deleteRule));
    } catch (e) {
      show({ message: errorMessage(t, e) });
      return;
    } finally {
      refresh();
    }
    show({
      message: t(ids.length > 1 ? 'categories.rulesDeleted' : 'categories.ruleDeleted'),
      undo: () => {
        Promise.all(ids.map(restoreRule))
          .catch((e) => show({ message: errorMessage(t, e) }))
          .finally(refresh);
      },
    });
  };

  const rows = rules ? ruleRows(rules, t) : [];
  return (
    <div>
      <span className="cg-k" id={headId}>{t('categories.rules')}</span>
      <div className="cg-rules">
        <ul className="cg-rules__list" aria-labelledby={headId}>
          {rows.map((r) => (
            <li key={r.ids.join(' ')} className="cg-rule">
              <span className="cg-rule__text">
                {r.before}
                <span className={`cg-tok${r.mono ? ' cg-tok--v' : ''}`}>{r.value}</span>
                {r.after}
              </span>
              <span className="caption cg-muted">{t('categories.itemCount', { n: r.matchCount })}</span>
              <Button
                variant="quiet"
                size="sm"
                icon="trash"
                aria-label={`${t('categories.deleteRule')}: ${ruleText(r)}`}
                onClick={() => void remove(r.ids)}
              />
            </li>
          ))}
        </ul>
        {rules && rows.length === 0 ? <p className="caption cg-muted cg-m0">{t('categories.noRules')}</p> : null}
        {adding ? (
          <AddRuleForm
            categoryId={c.id}
            onDone={() => {
              setAdding(false);
              refresh();
            }}
            onCancel={() => setAdding(false)}
          />
        ) : (
          <Button variant="secondary" size="sm" icon="plus" className="cg-self-start" onClick={() => setAdding(true)}>
            {t('categories.addRule')}
          </Button>
        )}
      </div>
    </div>
  );
}

function AddRuleForm(p: { categoryId: string; onDone: () => void; onCancel: () => void }): ReactElement {
  const t = useT();
  const kindId = useId();
  const valueId = useId();
  const [kind, setKind] = useState<Rule['kind']>('domain');
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const changeKind = (k: Rule['kind']) => {
    setKind(k);
    setValue(k === 'item_kind' ? ITEM_KINDS[0] : k === 'source_kind' ? SOURCE_KINDS[0] : '');
    setError(null);
  };

  const submit = async () => {
    if (!value.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await addRule(p.categoryId, kind, value);
      p.onDone();
    } catch (e) {
      setError(errorMessage(t, e));
      setBusy(false);
    }
  };

  const options =
    kind === 'item_kind'
      ? ITEM_KINDS.map((k) => ({ value: k, label: t(`kind.${k}`) }))
      : kind === 'source_kind'
        ? SOURCE_KINDS.map((k) => ({ value: k, label: t(`home.source.${k}`) }))
        : null;

  return (
    <form
      className="cg-addrule"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
      onKeyDown={(e) => e.key === 'Escape' && p.onCancel()}
    >
      <div>
        <label className="cg-k" htmlFor={kindId}>{t('categories.ruleKind')}</label>
        <select id={kindId} className="st-input" value={kind} autoFocus onChange={(e) => changeKind(e.target.value as Rule['kind'])}>
          {RULE_KINDS.map((k) => (
            <option key={k} value={k}>{t(`categories.kind.${k}`)}</option>
          ))}
        </select>
      </div>
      <div className="cg-grow">
        <label className="cg-k" htmlFor={valueId}>{t('categories.ruleValue')}</label>
        {options ? (
          <select id={valueId} className="st-input cg-input" value={value} onChange={(e) => setValue(e.target.value)}>
            {options.map((o) => (
              <option key={o.value} value={o.value}>{o.label}</option>
            ))}
          </select>
        ) : (
          <input
            id={valueId}
            className={`st-input cg-input${kind === 'regex' ? ' cg-mono' : ''}`}
            value={value}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={error ? true : undefined}
            onChange={(e) => setValue(e.target.value)}
          />
        )}
        {error ? <p className="st-error" role="alert">{error}</p> : null}
      </div>
      <div className="cg-addrule__actions">
        <Button type="button" variant="quiet" size="sm" onClick={p.onCancel}>{t('common.cancel')}</Button>
        <Button type="submit" variant="secondary" size="sm" disabled={!value.trim() || busy}>{t('categories.add')}</Button>
      </div>
    </form>
  );
}

function Examples({ category: c }: { category: Category }): ReactElement {
  const t = useT();
  const lang = useLang();
  const qc = useQueryClient();
  const { show } = useToast();
  const { data: examples } = useQuery({ queryKey: ['examples', c.id], queryFn: () => listExamples(c.id) });
  const [open, setOpen] = useState(false);
  const [notRuleable, setNotRuleable] = useState<ReadonlySet<string>>(new Set());
  const listId = useId();
  const label = categoryLabel(c, t);

  const makeRule = async (itemId: string) => {
    try {
      await makeRuleFromItem(itemId);
      show({ message: t('home.ruleSaved', { category: label }) });
      void qc.invalidateQueries({ queryKey: ['rules'] });
    } catch (e) {
      if (errorText(e) === 'notRuleable') setNotRuleable((s) => new Set(s).add(itemId));
      else show({ message: errorMessage(t, e) });
    }
  };

  const remove = (id: string) => {
    deleteExample(id)
      .catch((e) => show({ message: errorMessage(t, e) }))
      .finally(() => void qc.invalidateQueries({ queryKey: ['examples', c.id] }));
  };

  return (
    <div className="cg-box">
      <b>{t('categories.learned', { n: examples?.length ?? 0 })}</b>
      <div className="cg-box__row">
        <span className="caption cg-muted cg-grow">{t('categories.learnedCaption')}</span>
        <Button
          variant="secondary"
          size="sm"
          aria-expanded={open}
          aria-controls={listId}
          disabled={!examples?.length}
          onClick={() => setOpen((o) => !o)}
        >
          {t('categories.reviewExamples')}
        </Button>
      </div>
      {open && examples?.length ? (
        <ul className="cg-examples" id={listId}>
          {examples.map((ex) => {
            const blocked = notRuleable.has(ex.itemId);
            return (
              <li key={ex.id} className="cg-example">
                <span className="cg-example__title">{ex.title}</span>
                <span className="caption cg-muted">{formatDate(ex.createdAt, lang)}</span>
                <span title={blocked ? t('error.notRuleable') : undefined}>
                  <Button variant="secondary" size="sm" disabled={blocked} onClick={() => void makeRule(ex.itemId)}>
                    {t('categories.makeRule')}
                  </Button>
                </span>
                <Button variant="quiet" size="sm" onClick={() => remove(ex.id)}>{t('common.remove')}</Button>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}

function DeleteDialog(p: { category: Category; label: string; count: number; onClose: () => void }): ReactElement {
  const t = useT();
  const { show } = useToast();
  const [busy, setBusy] = useState(false);
  const confirm = async () => {
    setBusy(true);
    try {
      await deleteCategory(p.category.id);
      show({ message: t('categories.deleted', { name: p.label }) });
      navigate('categories');
    } catch (e) {
      show({ message: errorMessage(t, e) });
      setBusy(false);
      p.onClose();
    }
  };
  return (
    <Dialog
      title={t('categories.deleteTitle', { name: p.label })}
      role="alertdialog"
      onClose={p.onClose}
      actions={
        <>
          <Button variant="quiet" autoFocus onClick={p.onClose}>{t('common.cancel')}</Button>
          <Button variant="secondary" disabled={busy} onClick={() => void confirm()}>{t('categories.delete')}</Button>
        </>
      }
    >
      <p className="body cg-m0">{t('categories.deleteBody', { n: p.count })}</p>
    </Dialog>
  );
}

function NewCategoryDialog({ onClose }: { onClose: () => void }): ReactElement {
  const t = useT();
  const nameId = useId();
  const iconId = useId();
  const colourId = useId();
  const [name, setName] = useState('');
  const [icon, setIcon] = useState<IconName>('note');
  const [color, setColor] = useState<string>(SWATCHES[0]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const create = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const c = await createCategory(name.trim(), icon, color);
      onClose();
      navigate(`categories/${encodeURIComponent(c.id)}`);
    } catch (e) {
      setError(errorMessage(t, e));
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={t('categories.new')}
      onClose={onClose}
      actions={
        <>
          <Button variant="quiet" onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant="primary" disabled={!name.trim() || busy} onClick={() => void create()}>
            {t('categories.create')}
          </Button>
        </>
      }
    >
      <label className="st-row__label" htmlFor={nameId}>{t('categories.name')}</label>
      <input
        id={nameId}
        className="st-input"
        value={name}
        autoFocus
        autoComplete="off"
        aria-invalid={error ? true : undefined}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === 'Enter' && void create()}
      />
      {error ? <p className="st-error" role="alert">{error}</p> : null}
      <span className="st-row__label" id={iconId}>{t('categories.icon')}</span>
      <IconPicker value={icon} labelledBy={iconId} onChange={setIcon} />
      <span className="st-row__label" id={colourId}>{t('categories.colour')}</span>
      <ColourPicker value={color} labelledBy={colourId} onChange={setColor} />
    </Dialog>
  );
}
