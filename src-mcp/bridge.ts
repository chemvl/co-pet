// The Unix socket to Co-pet: one JSON line per call (`{id, client, tool, args}`), answered by
// `{id, ok: true, result}` or `{id, ok: false, error, message}` in completion order.
import { createConnection, type Socket } from 'node:net';
import { setTimeout as sleep } from 'node:timers/promises';

export const NOT_RUNNING = "Co-pet isn't running. Open Co-pet and try again.";

/// Co-pet rejects request lines longer than this (bytes, without the newline).
const MAX_LINE = 1024 * 1024;

/// Display names of the clients Co-pet's "Add to …" buttons configure.
export const KNOWN_CLIENTS: Record<string, string> = {
  'claude-code': 'Claude Code',
  cursor: 'Cursor',
  codex: 'Codex',
  'gemini-cli': 'Gemini CLI',
};

/// Who makes a call; `token` is set in HTTP mode only.
export interface ClientRef {
  key: string;
  name: string;
  token: string | null;
}

/// A failed call: Co-pet's error code (or `notRunning`) and an English sentence for the agent.
export class BridgeError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

/// `COPET_SOCKET`, else the socket in Co-pet's app data folder.
export function socketPath(env: Record<string, string | undefined> = process.env): string {
  return env.COPET_SOCKET ?? `${env.HOME}/Library/Application Support/com.copet.app/mcp.sock`;
}

function slug(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60)
    .replace(/-+$/, '');
}

/// A stdio client: `envKey` (`COPET_CLIENT`) if set, else `other:<slug of clientInfo.name>`;
/// the known display name for the four configured keys, else `clientInfo.name`.
export function stdioClient(envKey: string | undefined, clientName: string | undefined): ClientRef {
  const name = clientName?.trim() || 'Unknown client';
  const key = envKey?.trim() || `other:${slug(name) || 'unknown'}`;
  return { key, name: (KNOWN_CLIENTS[key] ?? name).slice(0, 100), token: null };
}

interface Pending {
  resolve: (result: unknown) => void;
  reject: (e: BridgeError) => void;
}

export class Bridge {
  private socket: Socket | null = null;
  private connecting: Promise<Socket> | null = null;
  private readonly pending = new Map<number, Pending>();
  private nextId = 1;
  private buffer = '';

  constructor(private readonly path: string) {}

  /// Runs one tool in Co-pet. Rejects with a `BridgeError`.
  async call(client: ClientRef, tool: string, args: unknown): Promise<unknown> {
    const id = this.nextId++;
    const line = JSON.stringify({ id, client, tool, args: args ?? {} });
    if (Buffer.byteLength(line) > MAX_LINE) throw new BridgeError('invalid', 'Request too large.');
    const socket = await this.connected();
    const { promise, resolve, reject } = Promise.withResolvers<unknown>();
    this.pending.set(id, { resolve, reject });
    socket.ref();
    socket.write(`${line}\n`);
    return promise;
  }

  /// The open socket; connects (and once more on `ECONNREFUSED`/`ENOENT`) when there is none.
  private connected(): Promise<Socket> {
    if (this.socket) return Promise.resolve(this.socket);
    this.connecting ??= this.open()
      .catch(async (e: unknown) => {
        const code = (e as NodeJS.ErrnoException).code;
        if (code !== 'ECONNREFUSED' && code !== 'ENOENT') throw e;
        await sleep(200);
        return this.open();
      })
      .catch(() => {
        throw new BridgeError('notRunning', NOT_RUNNING);
      })
      .finally(() => {
        this.connecting = null;
      });
    return this.connecting;
  }

  private open(): Promise<Socket> {
    const { promise, resolve, reject } = Promise.withResolvers<Socket>();
    const socket = createConnection(this.path);
    socket.once('error', reject);
    socket.once('connect', () => {
      socket.off('error', reject);
      this.attach(socket);
      resolve(socket);
    });
    return promise;
  }

  private attach(socket: Socket) {
    this.socket = socket;
    this.buffer = '';
    socket.setEncoding('utf8');
    socket.on('data', (chunk: string) => {
      this.buffer += chunk;
      let nl: number;
      while ((nl = this.buffer.indexOf('\n')) >= 0) {
        const line = this.buffer.slice(0, nl);
        this.buffer = this.buffer.slice(nl + 1);
        if (line.trim()) this.reply(line);
      }
    });
    socket.on('error', () => {});
    socket.on('close', () => {
      if (this.socket === socket) this.socket = null;
      this.failAll(new BridgeError('notRunning', NOT_RUNNING));
    });
    // Idle, the socket must not keep a stdio server alive after its client has gone.
    socket.unref();
  }

  private reply(line: string) {
    let msg: { id?: unknown; ok?: boolean; result?: unknown; error?: string; message?: string };
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    const err = msg.ok ? null : new BridgeError(msg.error ?? 'internal', msg.message ?? 'Co-pet failed.');
    const pending = typeof msg.id === 'number' ? this.pending.get(msg.id) : undefined;
    if (!pending) {
      // A reply Co-pet couldn't tie to a request (bad line): fail everything waiting.
      if (err && msg.id == null) this.failAll(err);
      return;
    }
    this.pending.delete(msg.id as number);
    if (err) pending.reject(err);
    else pending.resolve(msg.result);
    if (this.pending.size === 0) this.socket?.unref();
  }

  private failAll(e: BridgeError) {
    const waiting = [...this.pending.values()];
    this.pending.clear();
    for (const p of waiting) p.reject(e);
  }
}

export function connect(sockPath: string = socketPath()): Bridge {
  return new Bridge(sockPath);
}
