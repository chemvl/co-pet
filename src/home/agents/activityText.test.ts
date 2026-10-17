import { describe, expect, it } from 'vitest';
import { translate, type Lang, type TFunction } from '../../shared/i18n';
import type { ActivityRow, Category } from '../../shared/types';
import { activityAction, activityText, relativeTime } from './activityText';

const tr = (lang: Lang): TFunction => (key, params) => translate(lang, key, params);
const en = tr('en');
const vi = tr('vi');

function row(tool: string, detail: Record<string, unknown>, over: Partial<ActivityRow> = {}): ActivityRow {
  return {
    id: 'a1',
    at: 0,
    clientName: 'Claude Code',
    tool,
    isWrite: false,
    detail,
    resultUrl: null,
    error: null,
    undoneAt: null,
    undoable: false,
    ...over,
  };
}

const design: Category = {
  id: 'design',
  name: null,
  icon: null,
  color: null,
  position: 0,
  excludeFromAi: false,
  excludeFromMcp: false,
  builtin: true,
};

describe('activityText', () => {
  it('formats reads', () => {
    expect(activityText(row('search_items', { query: 'figma', count: 6 }), en)).toBe('Searched “figma” · 6 results');
    expect(activityText(row('search_items', { query: 'figma', count: 1 }), en)).toBe('Searched “figma” · 1 result');
    expect(activityText(row('search_items', { count: 0 }), en)).toBe('Listed items · 0 results');
    expect(activityText(row('get_item', { id: 'i1', title: 'useDebouncedValue hook' }), en)).toBe('Read “useDebouncedValue hook”');
    expect(activityText(row('list_tasks', { count: 3 }), en)).toBe('Listed tasks · 3 results');
    expect(activityText(row('whats_next', { id: 't1', title: 'Ship it' }), en)).toBe('Asked what’s next: “Ship it”');
    expect(activityText(row('whats_next', { count: 0 }), en)).toBe('Asked what’s next · nothing to suggest');
  });

  it('formats meeting reads', () => {
    expect(activityText(row('list_meetings', { count: 4 }), en)).toBe('Listed meetings · 4 results');
    expect(activityText(row('list_meetings', { count: 4 }), vi)).toBe('Liệt kê cuộc họp · 4 kết quả');
    expect(activityText(row('list_meetings', { query: 'release', count: 1 }), en)).toBe(
      'Searched meetings for “release” · 1 result',
    );
    expect(activityText(row('list_meetings', { query: 'release', count: 2 }), vi)).toBe(
      'Tìm cuộc họp “release” · 2 kết quả',
    );
    expect(activityText(row('get_meeting', { id: 'm1', title: 'Standup' }), en)).toBe('Read meeting “Standup”');
    expect(activityText(row('get_meeting', { id: 'm1', title: 'Standup' }), vi)).toBe('Đọc cuộc họp “Standup”');
  });

  it('formats writes', () => {
    expect(activityText(row('add_item', { title: 'Test plan' }), en)).toBe('Added “Test plan”');
    expect(activityText(row('create_task', { title: 'Review PR 212' }), en)).toBe('Created task “Review PR 212”');
    expect(activityText(row('update_task', { title: 'Review PR 212' }), en)).toBe('Updated task “Review PR 212”');
    expect(activityText(row('complete_task', { title: 'Review PR 212' }), en)).toBe('Completed “Review PR 212”');
    expect(activityText(row('export_task', { title: 'Pagination', provider: 'notion' }), en)).toBe(
      'Exported “Pagination” to Notion',
    );
    expect(activityText(row('update_item', { title: 'Logo', archived: true }), en)).toBe('Archived “Logo”');
    expect(activityText(row('update_item', { title: 'Logo', archived: false }), en)).toBe('Moved “Logo” back to the inbox');
    expect(activityText(row('update_item', { title: 'Logo', tags: ['brand'] }), en)).toBe('Tagged “Logo”');
    expect(activityText(row('update_item', { title: 'Logo' }), en)).toBe('Updated “Logo”');
    expect(activityText(row('mystery_tool', {}), en)).toBe('Called mystery_tool');
  });

  it('names the category of a move: live in the current language, else as logged', () => {
    const moved = row('update_item', { title: 'Logo', category: 'design', categoryName: 'Thiết kế' });
    expect(activityText(moved, en, [design])).toBe('Moved “Logo” to Design');
    const gone = row('update_item', { title: 'Logo', category: 'c9', categoryName: 'Clients' });
    expect(activityText(gone, en, [design])).toBe('Moved “Logo” to Clients');
  });

  it('formats errors', () => {
    expect(activityText(row('create_task', {}, { isWrite: true, error: 'writesNotApproved' }), en)).toBe(
      'Blocked: writes not approved',
    );
    expect(activityText(row('search_items', {}, { error: 'paused' }), en)).toBe('Blocked: server paused');
    expect(activityText(row('get_item', {}, { error: 'internal' }), en)).toBe('Failed: internal');
    expect(activityText(row('get_item', {}, { error: 'notFound' }), en)).toBe("Failed: It's no longer here.");
  });

  it('formats Vietnamese', () => {
    expect(activityText(row('search_items', { query: 'figma', count: 6 }), vi)).toBe('Đã tìm “figma” · 6 kết quả');
    expect(activityText(row('add_item', { title: 'Test plan' }), vi)).toBe('Đã thêm “Test plan”');
    expect(activityText(row('update_item', { title: 'Logo', category: 'design' }), vi, [design])).toBe(
      'Đã chuyển “Logo” sang Thiết kế',
    );
    expect(activityText(row('create_task', { title: 'Review PR 212' }), vi)).toBe('Đã tạo công việc “Review PR 212”');
    expect(activityText(row('complete_task', { title: 'Review PR 212' }), vi)).toBe('Đã hoàn thành “Review PR 212”');
    expect(activityText(row('export_task', { title: 'Pagination', provider: 'asana' }), vi)).toBe(
      'Đã xuất “Pagination” sang Asana',
    );
    expect(activityText(row('create_task', {}, { isWrite: true, error: 'writesNotApproved' }), vi)).toBe(
      'Bị chặn: chưa duyệt quyền ghi',
    );
  });
});

describe('activityAction', () => {
  it('picks the last cell', () => {
    expect(activityAction(row('search_items', { count: 1 }))).toEqual({ kind: 'read' });
    expect(activityAction(row('add_item', {}, { isWrite: true, undoable: true }))).toEqual({ kind: 'undo' });
    expect(activityAction(row('add_item', {}, { isWrite: true, undoable: true, undoneAt: 5 }))).toEqual({ kind: 'undone' });
    // A blocked write changed nothing.
    expect(activityAction(row('create_task', {}, { isWrite: true, error: 'writesNotApproved' }))).toBeNull();
    const exported = row('export_task', { provider: 'asana' }, { isWrite: true, resultUrl: 'https://app.asana.com/x' });
    expect(activityAction(exported)).toEqual({ kind: 'open', url: 'https://app.asana.com/x', provider: 'asana' });
    expect(activityAction(row('export_task', { provider: 'asana' }, { isWrite: true }))).toBeNull();
  });
});

describe('relativeTime', () => {
  const now = new Date(2026, 9, 17, 12, 0).getTime();
  it('counts minutes and hours, then shows the date', () => {
    expect(relativeTime(now - 30_000, now, en, 'en')).toBe('just now');
    expect(relativeTime(now - 2 * 60_000, now, en, 'en')).toBe('2 min ago');
    expect(relativeTime(now - 3 * 3_600_000, now, vi, 'vi')).toBe('3 giờ trước');
    expect(relativeTime(new Date(2026, 9, 14, 9, 0).getTime(), now, en, 'en')).toBe('Wed 14 Oct');
  });
});
