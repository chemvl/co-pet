import { describe, expect, it } from 'vitest';
import { stdioClient } from './bridge';
import { argSchemas } from './tools';

describe('tool argument schemas', () => {
  it('add_item rejects two sources and accepts each single one', () => {
    expect(argSchemas.add_item.safeParse({ text: 'note', url: 'https://figma.com' }).success).toBe(false);
    expect(argSchemas.add_item.safeParse({ title: 'only a title' }).success).toBe(false);
    for (const source of [{ text: 'note' }, { url: 'https://figma.com' }, { filePath: '/tmp/a.pdf' }]) {
      expect(argSchemas.add_item.safeParse(source).success).toBe(true);
    }
  });

  it('update_task accepts due: null', () => {
    const parsed = argSchemas.update_task.safeParse({ id: 't1', due: null });
    expect(parsed.success).toBe(true);
    expect(parsed.data).toEqual({ id: 't1', due: null });
  });
});

describe('stdioClient', () => {
  it('uses COPET_CLIENT and the known display name', () => {
    expect(stdioClient('claude-code', 'claude-ai')).toEqual({ key: 'claude-code', name: 'Claude Code', token: null });
  });

  it('falls back to other:<slug> of clientInfo.name', () => {
    expect(stdioClient(undefined, 'My Agent 2.0')).toEqual({ key: 'other:my-agent-2-0', name: 'My Agent 2.0', token: null });
  });
});
