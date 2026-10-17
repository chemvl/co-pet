// The twelve Co-pet tools and the `item://{id}` and `meeting://{id}` resources. Every call goes
// to Co-pet, which checks permissions and visibility, does the work and logs it.
import { type McpServer, ResourceTemplate } from '@modelcontextprotocol/sdk/server/mcp.js';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import { z } from 'zod';

/// Runs one tool in Co-pet for the current client; rejects with an `Error` whose message is
/// meant for the agent.
export type Call = (tool: string, args: unknown) => Promise<unknown>;

const UNDO = 'Changes can be undone by the user in Co-pet.';
const ONE_SOURCE = 'Give exactly one of text, url or filePath.';

const id = z.string().min(1);
const due = z.string().describe('2026-10-17, or 2026-10-17T14:30 for a time.');
const priority = z.number().int().min(0).max(3).describe('0 none to 3 high.');
const estimateMin = z.number().int().min(1).max(1440).describe('Estimate in minutes.');
const date = z.string().describe('A date like 2026-10-17.');

/// Argument shapes, as Co-pet's socket API takes them.
export const shapes = {
  search_items: {
    query: z.string().optional().describe('Words to look for; omit for the most recent items.'),
    category: z.string().optional().describe('Category id.'),
    since: z.string().optional().describe('Only items captured on or after this date, like 2026-10-17.'),
    limit: z.number().int().min(1).max(50).optional().describe('1 to 50, default 20.'),
  },
  get_item: {
    id,
    format: z.enum(['markdown', 'text', 'path']).optional().describe('Default markdown; path needs a file item.'),
  },
  list_tasks: {
    status: z.enum(['open', 'done']).optional().describe('Default open.'),
    due: z.enum(['overdue', 'today', 'week']).optional(),
    limit: z.number().int().min(1).max(100).optional().describe('1 to 100, default 50.'),
  },
  whats_next: {},
  list_meetings: {
    query: z.string().optional().describe('Words that must all appear in the title or summary.'),
    since: date.optional().describe('Only meetings started on or after this date, like 2026-10-17.'),
    limit: z.number().int().min(1).max(50).optional().describe('1 to 50, default 20.'),
  },
  get_meeting: {
    id,
    format: z.enum(['markdown', 'json']).optional().describe('Default markdown.'),
    transcript: z.boolean().optional().describe('Default true'),
  },
  add_item: {
    text: z.string().optional(),
    url: z.string().optional(),
    filePath: z.string().optional().describe('Absolute path of an existing file.'),
    title: z.string().max(300).optional(),
  },
  update_item: {
    id,
    category: z.string().optional().describe('Category id.'),
    tags: z.array(z.string().min(1).max(50)).max(10).optional().describe('Replaces every tag.'),
    archive: z.boolean().optional(),
  },
  create_task: {
    title: z.string().min(1).max(300),
    notes: z.string().optional(),
    due: due.optional(),
    estimateMin: estimateMin.optional(),
    priority: priority.optional(),
    sourceItemId: z.string().optional().describe('The item this task comes from.'),
  },
  update_task: {
    id,
    title: z.string().min(1).max(300).optional(),
    notes: z.string().optional(),
    due: due.nullable().optional().describe('2026-10-17 or 2026-10-17T14:30; null clears it.'),
    estimateMin: estimateMin.optional(),
    priority: priority.optional(),
  },
  complete_task: { id },
  export_task: { id, provider: z.enum(['notion', 'asana']) },
} as const;

export type ToolName = keyof typeof shapes;

/// Full argument schemas: the shapes, plus `add_item`'s one-source rule (an MCP input schema is
/// a plain object, so the handler applies that rule).
export const argSchemas = {
  search_items: z.object(shapes.search_items),
  get_item: z.object(shapes.get_item),
  list_tasks: z.object(shapes.list_tasks),
  whats_next: z.object(shapes.whats_next),
  list_meetings: z.object(shapes.list_meetings),
  get_meeting: z.object(shapes.get_meeting),
  add_item: z
    .object(shapes.add_item)
    .refine((a) => [a.text, a.url, a.filePath].filter((v) => v !== undefined).length === 1, ONE_SOURCE),
  update_item: z.object(shapes.update_item),
  create_task: z.object(shapes.create_task),
  update_task: z.object(shapes.update_task),
  complete_task: z.object(shapes.complete_task),
  export_task: z.object(shapes.export_task),
} satisfies Record<ToolName, z.ZodType>;

const descriptions: Record<ToolName, string> = {
  search_items: 'Searches the items saved in Co-pet, newest first.',
  get_item: "Returns one item's full content.",
  list_tasks: "Lists Co-pet's tasks, filtered by status and due date.",
  whats_next: 'Returns the task Co-pet suggests doing next, or null.',
  list_meetings: 'Lists recorded meetings with their summary, newest first.',
  get_meeting: "Returns one meeting's summary, decisions, action items, open questions and transcript.",
  add_item: `Saves text, a URL or a file to Co-pet as a new item. ${UNDO}`,
  update_item: `Moves an item to a category, replaces its tags or archives it. ${UNDO}`,
  create_task: `Creates a task in Co-pet. ${UNDO}`,
  update_task: `Changes a task's title, notes, due date, estimate or priority. ${UNDO}`,
  complete_task: `Marks a task done. ${UNDO}`,
  export_task: `Sends a task to Notion or Asana. ${UNDO}`,
};

/// Registers the tools and the `item://{id}` and `meeting://{id}` resource templates on `server`.
export function registerTools(server: McpServer, call: Call) {
  for (const name of Object.keys(shapes) as ToolName[]) {
    server.tool(name, descriptions[name], shapes[name], async (args: unknown): Promise<CallToolResult> => {
      try {
        const checked = argSchemas[name].safeParse(args);
        if (!checked.success) throw new Error(checked.error.issues[0]?.message ?? 'Invalid arguments.');
        const result = await call(name, args);
        return { content: [{ type: 'text', text: JSON.stringify(result, null, 2) }] };
      } catch (e) {
        return { isError: true, content: [{ type: 'text', text: e instanceof Error ? e.message : String(e) }] };
      }
    });
  }

  server.registerResource(
    'item',
    new ResourceTemplate('item://{id}', {
      list: async () => {
        const items = (await call('search_items', { limit: 50 })) as { id: string; title: string }[];
        return {
          resources: items.map((i) => ({ uri: `item://${i.id}`, name: i.title, mimeType: 'text/markdown' })),
        };
      },
    }),
    { description: 'A Co-pet item as markdown.', mimeType: 'text/markdown' },
    async (uri, { id }) => {
      const item = (await call('get_item', { id: String(id), format: 'markdown' })) as { content: string };
      return { contents: [{ uri: uri.href, mimeType: 'text/markdown', text: item.content }] };
    },
  );

  server.registerResource(
    'meeting',
    new ResourceTemplate('meeting://{id}', {
      list: async () => {
        const meetings = (await call('list_meetings', { limit: 50 })) as { id: string; title: string }[];
        return {
          resources: meetings.map((m) => ({ uri: `meeting://${m.id}`, name: m.title, mimeType: 'text/markdown' })),
        };
      },
    }),
    { description: 'A Co-pet meeting as markdown.', mimeType: 'text/markdown' },
    async (uri, { id }) => {
      const text = (await call('get_meeting', { id: String(id), format: 'markdown' })) as string;
      return { contents: [{ uri: uri.href, mimeType: 'text/markdown', text }] };
    },
  );
}
