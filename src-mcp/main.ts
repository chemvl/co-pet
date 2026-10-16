// `copet-mcp --stdio` serves one agent over stdin/stdout; `copet-mcp --http --port <n>` (run by
// Co-pet) serves token-holding agents at http://127.0.0.1:<n>/mcp. Both forward every call to
// Co-pet over its Unix socket.
import { createServer } from 'node:http';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import pkg from '../package.json';
import { connect, stdioClient, type ClientRef } from './bridge';
import { registerTools } from './tools';

/// Exit code when the HTTP port is taken; Co-pet then shows the port as busy and stops retrying.
const EXIT_PORT_IN_USE = 3;
const USAGE = 'Usage: copet-mcp --stdio | copet-mcp --http --port <n>';

const bridge = connect();

function newServer(client: () => ClientRef): McpServer {
  const server = new McpServer({ name: 'copet', version: pkg.version });
  registerTools(server, (tool, args) => bridge.call(client(), tool, args));
  return server;
}

async function serveStdio() {
  const server = newServer(() => stdioClient(process.env.COPET_CLIENT, server.server.getClientVersion()?.name));
  await server.connect(new StdioServerTransport());
}

/// Stateless: each POST gets its own server, bound to that request's bearer token.
function serveHttp(port: number) {
  const http = createServer(async (req, res) => {
    if (new URL(req.url ?? '/', 'http://127.0.0.1').pathname !== '/mcp') {
      res.writeHead(404).end();
      return;
    }
    const token = /^Bearer\s+(\S+)$/i.exec(req.headers.authorization ?? '')?.[1];
    if (!token) {
      res
        .writeHead(401, { 'content-type': 'application/json', 'www-authenticate': 'Bearer' })
        .end(JSON.stringify({ error: 'unauthorized', message: 'Send Authorization: Bearer <token>. Create a token in Co-pet › Agents.' }));
      return;
    }
    const server = newServer(() => ({ key: 'http', name: 'HTTP client', token }));
    const transport = new StreamableHTTPServerTransport({ sessionIdGenerator: undefined });
    res.on('close', () => {
      void transport.close();
      void server.close();
    });
    try {
      await server.connect(transport);
      await transport.handleRequest(req, res);
    } catch (e) {
      console.error('[copet-mcp] request failed:', e);
      if (!res.headersSent) res.writeHead(500).end();
    }
  });
  http.on('error', (e: NodeJS.ErrnoException) => {
    console.error(`[copet-mcp] http: ${e.message}`);
    process.exit(e.code === 'EADDRINUSE' ? EXIT_PORT_IN_USE : 1);
  });
  http.listen(port, '127.0.0.1');
}

const argv = process.argv.slice(2);
if (argv.includes('--stdio')) {
  await serveStdio();
} else if (argv.includes('--http')) {
  const port = Number(argv[argv.indexOf('--port') + 1]);
  if (!argv.includes('--port') || !Number.isInteger(port) || port < 1 || port > 65535) {
    console.error(USAGE);
    process.exit(2);
  }
  serveHttp(port);
} else {
  console.error(USAGE);
  process.exit(2);
}
