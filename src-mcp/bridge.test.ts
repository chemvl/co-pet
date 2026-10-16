import { chmodSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { Bridge, BridgeError, connectError, NOT_RUNNING } from './bridge';

function errno(code: string, message: string): NodeJS.ErrnoException {
  return Object.assign(new Error(message), { code });
}

describe('connectError', () => {
  it('maps a missing socket and a refused connection to "not running"', () => {
    for (const code of ['ENOENT', 'ECONNREFUSED']) {
      const e = connectError(errno(code, `connect ${code} /x/mcp.sock`));
      expect([e.code, e.message]).toEqual(['notRunning', NOT_RUNNING]);
    }
  });

  it('surfaces other failures with their message', () => {
    const e = connectError(errno('EACCES', 'connect EACCES /x/mcp.sock'));
    expect([e.code, e.message]).toEqual(['unreachable', "Can't reach Co-pet: connect EACCES /x/mcp.sock"]);
    expect(connectError(errno('EPERM', 'connect EPERM')).message).toBe("Can't reach Co-pet: connect EPERM");
  });
});

describe('Bridge connect', () => {
  let dir: string;
  afterEach(() => {
    chmodSync(dir, 0o700);
    rmSync(dir, { recursive: true, force: true });
  });

  const client = { key: 'cursor', name: 'Cursor', token: null };

  it('reports a missing socket as Co-pet not running', async () => {
    dir = mkdtempSync(join(tmpdir(), 'copet-bridge-'));
    const err = (await new Bridge(join(dir, 'mcp.sock')).call(client, 'search_items', {}).catch((e) => e)) as BridgeError;
    expect(err).toBeInstanceOf(BridgeError);
    expect(err.code).toBe('notRunning');
  });

  it('reports a socket it may not open as unreachable', async () => {
    dir = mkdtempSync(join(tmpdir(), 'copet-bridge-'));
    chmodSync(dir, 0);
    const err = (await new Bridge(join(dir, 'mcp.sock')).call(client, 'search_items', {}).catch((e) => e)) as BridgeError;
    expect(err).toBeInstanceOf(BridgeError);
    expect(err.code).toBe('unreachable');
    expect(err.message).toMatch(/^Can't reach Co-pet: .*EACCES/);
  });
});
