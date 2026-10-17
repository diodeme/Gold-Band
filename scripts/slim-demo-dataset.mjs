import { readFile, writeFile, mkdir, readdir, rm, stat, copyFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { join, resolve, dirname, relative } from 'node:path';

const sha256 = (buffer) => createHash('sha256').update(buffer).digest('hex');

// Long tool payloads dominate the export. The demo keeps the tool card title and a short
// preview, so any field carrying captured stdout/stderr is clipped to this budget.
const TRUNCATED_FIELDS = /^(?:rawOutput|toolOutput|output|stdout|stderr|content|rawInput|_meta|data|formatted_output|delta|payload)$/i;
const MARKER = '\n\n[演示数据已截断]';

function parseArguments(argv) {
  const options = {
    source: process.env.DEMO_SLIM_SOURCE ?? null,
    out: process.env.DEMO_SLIM_OUT ?? 'marketing/demo/data/ji-history',
    toolOutputChars: Number(process.env.DEMO_SLIM_TOOL_OUTPUT_CHARS ?? 600),
    toolDetail: process.env.DEMO_SLIM_TOOL_DETAIL ?? 'none',
    activityRows: (process.env.DEMO_SLIM_ACTIVITY ?? 'rows') === 'rows',
    dropNodeId: process.env.DEMO_SLIM_DROP_NODE ?? null,
    dropUserText: process.env.DEMO_SLIM_DROP_USER_TEXT ?? null,
  };
  for (let index = 0; index < argv.length; index += 2) {
    const [flag, value] = [argv[index], argv[index + 1]];
    if (value === undefined) throw new Error(`Missing value for ${flag}`);
    if (flag === '--source') options.source = value;
    else if (flag === '--out') options.out = value;
    else if (flag === '--tool-output-chars') options.toolOutputChars = Number(value);
    else if (flag === '--tool-detail') options.toolDetail = value;
    else if (flag === '--activity') options.activityRows = value === 'rows';
    else if (flag === '--drop-node') options.dropNodeId = value;
    else if (flag === '--drop-user-text') options.dropUserText = value;
    else throw new Error(`Unknown argument: ${flag}`);
  }
  if (!options.source) throw new Error('A source dataset is required: pass --source <dir> or set DEMO_SLIM_SOURCE.');
  if (!Number.isSafeInteger(options.toolOutputChars) || options.toolOutputChars < 0) throw new Error('--tool-output-chars must be a non-negative integer.');
  if (options.toolDetail !== 'none' && options.toolDetail !== 'preview') throw new Error('--tool-detail must be none or preview.');
  if (Boolean(options.dropNodeId) !== Boolean(options.dropUserText)) throw new Error('--drop-node and --drop-user-text must be set together.');
  return { ...options, source: resolve(options.source), out: resolve(options.out) };
}

async function exists(path) {
  return stat(path).then(() => true, (error) => { if (error.code === 'ENOENT') return false; throw error; });
}

async function directories(path) {
  return (await readdir(path, { withFileTypes: true })).filter((entry) => entry.isDirectory()).map((entry) => entry.name);
}

function clip(value, limit, stats) {
  if (typeof value !== 'string' || value.length <= limit) return value;
  stats.truncated += 1;
  stats.removedChars += value.length - limit;
  return `${value.slice(0, limit)}${MARKER}`;
}

function slimEvent(value, limit, stats) {
  if (Array.isArray(value)) return value.map((item) => slimEvent(item, limit, stats));
  if (!value || typeof value !== 'object') return value;
  const projected = {};
  for (const [key, child] of Object.entries(value)) {
    projected[key] = TRUNCATED_FIELDS.test(key) ? clipTree(child, limit, stats) : slimEvent(child, limit, stats);
  }
  return projected;
}

// Payload fields nest their text (for example rawOutput.formatted_output), so the
// budget applies to every string inside the subtree while structure stays intact.
function clipTree(value, limit, stats) {
  if (typeof value === 'string') return clip(value, limit, stats);
  if (Array.isArray(value)) return value.map((item) => clipTree(item, limit, stats));
  if (!value || typeof value !== 'object') return value;
  const projected = {};
  for (const [key, child] of Object.entries(value)) projected[key] = clipTree(child, limit, stats);
  return projected;
}

const goldBandMeta = (event) => event?.raw?._meta?.goldBandConversation ?? {};

// Tool detail payloads are not published, so the published cards must not advertise one.
// Otherwise the reader fetches a missing resource when the card is expanded.
function withoutToolDetail(event) {
  const meta = goldBandMeta(event);
  if (!event?.raw?._meta) return event;
  const { toolOutput: _toolOutput, toolDetailAvailable: _toolDetailAvailable, ...rest } = meta;
  return {
    ...event,
    raw: { ...event.raw, _meta: { ...event.raw._meta, goldBandConversation: { ...rest, toolDetailAvailable: false } } },
  };
}

// The audit list renders a title row per activity, so it needs the label, status, elapsed time
// and (for thoughts) the text, but not the raw payload or the unused timing envelope.
function activityRow(event) {
  const meta = goldBandMeta(event);
  const row = {
    id: event.id,
    kind: event.kind,
    title: event.title,
    status: event.status,
    toolCallId: event.toolCallId,
    sessionId: event.sessionId,
    seq: event.seq,
    startedSeq: event.startedSeq,
    endedSeq: event.endedSeq,
    timestamp: event.timestamp,
    durationMs: event.durationMs,
    raw: {
      _meta: { goldBandConversation: { branchId: meta.branchId, toolName: meta.toolName, toolDetailAvailable: false } },
    },
  };
  if (event.kind !== 'toolCall' && typeof event.content === 'string') row.content = event.content;
  return row;
}

function followUpCutoff(events, text) {
  return events.find((event) => event?.kind === 'userTextDelta' && typeof event.content === 'string' && event.content.includes(text)) ?? null;
}

function blocksBeforeFollowUp(blocks, eventsById, cutoffSeq, text) {
  const kept = [];
  for (const block of blocks) {
    if (block.summary) {
      const seq = block.summary.seq ?? block.oldestSeq;
      const content = block.summary.content;
      if (seq >= cutoffSeq || (typeof content === 'string' && content.includes(text))) continue;
      kept.push(block);
      continue;
    }
    const itemIds = (block.itemIds ?? []).filter((id) => {
      const event = eventsById.get(id);
      return event && (event.seq ?? event.startedSeq) < cutoffSeq;
    });
    if (itemIds.length === 0) continue;
    const seqs = itemIds.map((id) => eventsById.get(id).seq ?? eventsById.get(id).startedSeq);
    kept.push({ ...block, itemIds, oldestSeq: Math.min(...seqs), newestSeq: Math.max(...seqs) });
  }
  return kept;
}

async function main() {
  const { source, out, toolOutputChars, toolDetail, activityRows, dropNodeId, dropUserText } = parseArguments(process.argv.slice(2));
  const stats = { truncated: 0, removedChars: 0, sessions: 0, events: 0, droppedFiles: 0 };

  const publish = async (path, bytes) => {
    await mkdir(dirname(join(out, path)), { recursive: true });
    await writeFile(join(out, path), bytes);
    return { path, byteLength: bytes.length, sha256: sha256(bytes) };
  };
  const publishJson = (path, value) => publish(path, Buffer.from(JSON.stringify(value)));
  const copyThrough = async (path) => {
    const bytes = await readFile(join(source, path));
    return publish(path, bytes);
  };

  await rm(out, { recursive: true, force: true });
  await mkdir(out, { recursive: true });

  const dataset = JSON.parse(await readFile(join(source, 'dataset.json'), 'utf8'));
  const rootResources = await exists(join(source, 'resources.json'))
    ? JSON.parse(await readFile(join(source, 'resources.json'), 'utf8'))
    : { sessions: {} };
  rootResources.sessions ??= {};

  const run = await copyThrough('run.json');
  const task = await copyThrough('task.json');
  const workflow = await copyThrough('workflow.json');
  // The workspace file tree and file snapshots are dropped, so every session points at this
  // shared empty listing to keep the browser panel rendering an empty state.
  const emptyDirectory = await publishJson('empty-directory.json', []);

  const keptSessions = [];
  let droppedFollowUp = false;
  for (const reference of dataset.sessions) {
    const sessionDirectory = dirname(reference.path);
    const index = JSON.parse(await readFile(join(source, reference.path), 'utf8'));
    const sourceIndexDirectory = dirname(join(source, reference.path));

    for (const kind of ['images', 'changes', 'comparisons']) {
      const path = join(sourceIndexDirectory, kind);
      if (!(await exists(path))) continue;
      for (const file of await readdir(path, { withFileTypes: true })) {
        if (!file.isFile()) continue;
        await copyThrough(`${sessionDirectory}/${kind}/${file.name}`);
      }
    }

    const pagesDirectory = join(sourceIndexDirectory, 'pages');
    const sourcePages = [];
    if (await exists(pagesDirectory)) {
      const names = (await readdir(pagesDirectory)).sort((left, right) => Number.parseInt(left, 10) - Number.parseInt(right, 10));
      for (const name of names) sourcePages.push(JSON.parse(await readFile(join(pagesDirectory, name), 'utf8')));
    }
    const preparedPages = sourcePages.map((events) => events.map((event) => event?.kind === 'toolCall' ? withoutToolDetail(event) : event));
    const shouldDropFollowUp = dropNodeId && dropUserText && reference.nodeId === dropNodeId && reference.branchId === 'root';
    let publishedPages = preparedPages;
    let publishedBlocks = index.blocks;
    let followUpSeq = null;
    if (shouldDropFollowUp) {
      const cutoff = followUpCutoff(preparedPages.flat(), dropUserText);
      if (!cutoff) throw new Error(`User follow-up was not found in ${reference.nodeId}`);
      followUpSeq = cutoff.seq ?? cutoff.startedSeq;
      const keptEvents = preparedPages.flat().filter((event) => (event.seq ?? event.startedSeq) < followUpSeq);
      const eventsById = new Map(keptEvents.map((event) => [event.id, event]));
      publishedBlocks = blocksBeforeFollowUp(index.blocks, eventsById, followUpSeq, dropUserText);
      publishedPages = [];
      for (let offset = 0; offset < publishedBlocks.length; offset += 50) {
        const events = [];
        for (const block of publishedBlocks.slice(offset, offset + 50)) {
          if (block.summary) events.push(block.summary);
          else for (const id of block.itemIds) {
            const event = eventsById.get(id);
            if (!event) throw new Error(`Missing semantic item after follow-up cut: ${id}`);
            events.push(event);
          }
        }
        publishedPages.push(events);
      }
      droppedFollowUp = true;
    }
    const pageRefs = [];
    for (const [page, events] of publishedPages.entries()) {
      pageRefs.push(await publishJson(`${sessionDirectory}/pages/${page}.json`, events));
    }

    const eventRefs = {};
    const activityPages = [];
    const activityDirectory = join(sourceIndexDirectory, 'activity');
    if (activityRows && await exists(activityDirectory)) {
      const names = (await readdir(activityDirectory)).sort((left, right) => Number.parseInt(left, 10) - Number.parseInt(right, 10));
      const activityEvents = [];
      for (const name of names) {
        const events = JSON.parse(await readFile(join(activityDirectory, name), 'utf8'));
        activityEvents.push(...events);
      }
      const visibleActivity = followUpSeq == null
        ? activityEvents
        : activityEvents.filter((event) => (event.seq ?? event.startedSeq) < followUpSeq);
      for (let offset = 0; offset < visibleActivity.length; offset += 50) {
        const page = activityPages.length;
        const rows = visibleActivity.slice(offset, offset + 50).map(activityRow);
        activityPages.push(await publishJson(`${sessionDirectory}/activity/${page}.json`, rows));
        for (const row of rows) {
          eventRefs[row.id] = {
            id: row.id,
            kind: row.kind,
            seq: row.seq,
            startedSeq: row.startedSeq,
            endedSeq: row.endedSeq,
            branchId: goldBandMeta(row).branchId,
            activityPage: page,
          };
          stats.events += 1;
        }
      }
    }
    if (toolDetail === 'preview') {
      for (const [id, eventRef] of Object.entries(index.eventRefs ?? {})) {
        if (eventRef.kind !== 'toolCall') continue;
        const event = JSON.parse(await readFile(join(source, eventRef.path), 'utf8'));
        const entry = await publishJson(eventRef.path, slimEvent(event, toolOutputChars, stats));
        eventRefs[id] = { ...entry, kind: eventRef.kind, toolCallId: eventRef.toolCallId, branchId: eventRef.branchId };
      }
    }

    const directoryPath = join(sourceIndexDirectory, 'directories');
    if (await exists(directoryPath)) stats.droppedFiles += (await readdir(directoryPath)).length;

    const sessionResourcesPath = `${sessionDirectory}/resources.json`;
    const sourceResourcesPath = join(source, sessionResourcesPath);
    const changes = await exists(sourceResourcesPath)
      ? JSON.parse(await readFile(sourceResourcesPath, 'utf8')).changes ?? {}
      : {};
    const sessionResources = await publishJson(sessionResourcesPath, { changes, directory: emptyDirectory });

    const session = followUpSeq == null ? index.session : {
      ...index.session,
      eventPage: {
        ...index.session.eventPage,
        loadedCount: 0,
        total: publishedBlocks.length,
        hasOlder: publishedBlocks.length > 0,
        hasNewer: false,
      },
    };
    const slimIndex = {
      session,
      blocks: publishedBlocks,
      eventRefs,
      pages: pageRefs,
      activityPages,
    };
    const indexEntry = await publishJson(reference.path, slimIndex);
    keptSessions.push({
      ...reference,
      ...indexEntry,
      path: reference.path,
      ...(followUpSeq == null ? {} : {
        logicalCount: publishedBlocks.length,
        eventCount: publishedPages.flat().length,
      }),
    });
    rootResources.sessions[sessionDirectory] = { ...sessionResources, path: sessionResourcesPath };
    stats.sessions += 1;
  }
  if (dropNodeId && !droppedFollowUp) throw new Error(`User follow-up was not removed from ${dropNodeId}`);

  const slimDataset = {
    ...dataset,
    run,
    task,
    workflow,
    sessions: keptSessions,
  };
  const keptDirectories = new Set(keptSessions.map((session) => dirname(session.path)));
  const resourcesEntry = await publishJson('resources.json', {
    sessions: Object.fromEntries(Object.entries(rootResources.sessions).filter(([key]) => keptDirectories.has(key))),
  });
  slimDataset.resources = resourcesEntry;
  await publishJson('dataset.json', slimDataset);

  const inventory = [];
  const walk = async (path) => {
    for (const entry of await readdir(path, { withFileTypes: true })) {
      const child = join(path, entry.name);
      if (entry.isDirectory()) await walk(child);
      else {
        const bytes = await readFile(child);
        inventory.push({ path: relative(out, child).replaceAll('\\', '/'), byteLength: bytes.length, sha256: sha256(bytes) });
      }
    }
  };
  await walk(out);

  const manifestEntry = await publishJson('manifest.json', {
    version: dataset.version,
    projectId: dataset.projectId,
    taskId: dataset.taskId,
    taskUuid: dataset.taskUuid,
    runId: dataset.runId,
    title: dataset.title,
    source: dataset.source,
    sessions: keptSessions.map((session) => ({
      key: session.key ?? session.path,
      path: session.path,
      byteLength: session.byteLength,
      sha256: session.sha256,
      roundId: session.roundId,
      nodeId: session.nodeId,
      attemptId: session.attemptId,
      outerNodeId: session.outerNodeId,
      outerAttemptId: session.outerAttemptId,
      branchId: session.branchId,
      sessionId: session.sessionId,
    })),
    files: inventory,
    missing: [],
    externalDependencies: [],
    redactions: {},
    slim: { toolDetail, toolOutputChars, dropped: ['archive', 'files', 'directories', 'raw', 'activity'] },
  });

  // manifest.json is build-time bookkeeping: it stays in the repository dataset but is kept
  // out of the publication inventory so the deployed bundle does not carry it.
  await writeFile(join(out, 'publish-index.json'), JSON.stringify({ files: inventory }));

  const totalBytes = inventory.reduce((sum, entry) => sum + entry.byteLength, 0);
  console.log(JSON.stringify({
    out,
    sessions: stats.sessions,
    events: stats.events,
    files: inventory.length,
    publishedMiB: Number((totalBytes / 1024 / 1024).toFixed(1)),
    manifestMiB: Number((manifestEntry.byteLength / 1024 / 1024).toFixed(1)),
    totalMiB: Number(((totalBytes + manifestEntry.byteLength) / 1024 / 1024).toFixed(1)),
    truncatedFields: stats.truncated,
    removedMiB: Number((stats.removedChars / 1024 / 1024).toFixed(1)),
  }, null, 2));
}

await main();
