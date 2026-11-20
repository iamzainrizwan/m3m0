// m3m0 bridge: holds a websocket to the m3m0 app (which is the server, since a
// plugin can't listen) and applies the ops it sends through the plugin SDK, so
// RemNote writes the sync stamps itself. Protocol: docs/bridge.md.
import { declareIndexPlugin, type ReactRNPlugin, type RichTextInterface } from '@remnote/plugin-sdk';
import '../style.css';
import '../index.css';

const PORT = 47830;
const VERSION = 1;
let ws: WebSocket | null = null;
let stopped = false;

type Msg = { id: number; op: string; [k: string]: any };

async function rem(plugin: ReactRNPlugin, id: string) {
  const r = await plugin.rem.findOne(id);
  if (!r) throw new Error(`no rem ${id}`);
  return r;
}

async function apply(plugin: ReactRNPlugin, m: Msg): Promise<any> {
  switch (m.op) {
    case 'ping':
      return { version: VERSION };
    case 'create': {
      const r = await plugin.rem.createRem();
      if (!r) throw new Error('createRem returned nothing');
      try {
        await set(plugin, r._id, m);
      } catch (e) {
        // don't leave a half-made orphan rem behind
        await r.remove().catch(() => {});
        throw e;
      }
      return { id: r._id };
    }
    case 'update':
      await set(plugin, m.rem, m);
      return {};
    case 'delete':
      await (await rem(plugin, m.rem)).remove();
      return {};
    case 'get': {
      const r = await rem(plugin, m.rem);
      const cards = await r.getCards();
      return {
        text: r.text,
        back: r.backText,
        parent: r.parent,
        type: r.type,
        cards: cards.map((c) => ({ id: c._id, type: c.type, next: c.nextRepetitionTime, history: c.repetitionHistory })),
      };
    }
    case 'alias': {
      // [[name|alias]] refs: an alias rem of m.rem with text m.text (reused if equal)
      const r = await rem(plugin, m.rem);
      const a: any = await r.getOrCreateAliasWithText(m.text as RichTextInterface);
      let id: string | undefined = typeof a === 'string' ? a : a?._id;
      if (!id) {
        // the returned Rem can come back without an id: find the alias by its text
        const want = await plugin.richText.toString(m.text as RichTextInterface);
        for (const al of await r.getAliases()) {
          if ((await plugin.richText.toString(al.text ?? [])) === want) id = al._id;
        }
      }
      // no id: m3m0 reads it from the rem's `al` field in the DB
      return id ? { id } : {};
    }
    case 'review': {
      const c = await plugin.card.findOne(m.card);
      if (!c) throw new Error(`no card ${m.card}`);
      await c.updateCardRepetitionStatus(m.score);
      const after = await plugin.card.findOne(m.card);
      return { next: after?.nextRepetitionTime, history: after?.repetitionHistory };
    }
    default:
      throw new Error(`unknown op ${m.op}`);
  }
}

// fields shared by create/update; each is optional
async function set(plugin: ReactRNPlugin, id: string, m: Msg) {
  const r = await rem(plugin, id);
  if (m.parent !== undefined) await r.setParent(m.parent, m.position);
  if (m.text !== undefined) await r.setText(m.text as RichTextInterface);
  if (m.back !== undefined) await r.setBackText(m.back === null ? undefined : (m.back as RichTextInterface));
  if (m.remType !== undefined) await r.setType(m.remType);
  if (m.heading !== undefined) await r.setFontSize(m.heading ?? undefined);
  if (m.practice !== undefined) await r.setPracticeDirection(m.practice);
  if (m.cardItem !== undefined) await r.setIsCardItem(m.cardItem);
  if (m.document !== undefined) await r.setIsDocument(m.document);
  if (m.listItem !== undefined) await r.setIsListItem(m.listItem);
}

function connect(plugin: ReactRNPlugin) {
  if (stopped) return;
  const sock = new WebSocket(`ws://127.0.0.1:${PORT}`);
  ws = sock;
  sock.onopen = () => sock.send(JSON.stringify({ hello: 'm3m0-bridge', version: VERSION }));
  sock.onmessage = async (ev) => {
    let m: Msg;
    try {
      m = JSON.parse(ev.data);
    } catch {
      return;
    }
    try {
      const result = await apply(plugin, m);
      sock.send(JSON.stringify({ id: m.id, ok: true, result }));
    } catch (e: any) {
      sock.send(JSON.stringify({ id: m.id, ok: false, error: String(e?.message ?? e) }));
    }
  };
  sock.onclose = () => {
    if (ws === sock) ws = null;
    setTimeout(() => connect(plugin), 2000); // m3m0 not running yet: keep trying quietly
  };
  sock.onerror = () => sock.close();
}

async function onActivate(plugin: ReactRNPlugin) {
  stopped = false;
  connect(plugin);
}

async function onDeactivate(_: ReactRNPlugin) {
  stopped = true;
  ws?.close();
}

declareIndexPlugin(onActivate, onDeactivate);
