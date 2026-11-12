// Everything the console knows, and the connections that feed it.
//
// The console is a shell for several ghosts. Each ghost that is awake has her
// own daemon, and the console holds two connections to it: as a *console* it
// watches her status and her journal; as a *presence* it carries one channel,
// `console`, on which whoever is at this screen talks to her like anyone
// else. It cannot speak for her and has no way to change her.
//
// In the desktop app the keeper says which ghosts live on this machine and
// where each one is listening. In a plain web build there is no keeper, and
// the console talks to the one daemon it is pointed at.

import { useSyncExternalStore } from "react";
import { Link, type LinkState } from "./protocol/link";
import {
  DEFAULT_URL,
  PROTOCOL,
  type Action,
  type Agenda,
  type Entry,
  type Identity,
  type PersonRow,
  type Status,
} from "./protocol/types";
import { hasKeeper, keep, type Local, type LocalGhost, type Model } from "./shell";

export const CHANNEL = "console";
const REMOTE = "remote";

/** One ghost, as the console holds her. */
export type Session = {
  /** Her directory on this machine, or `remote`. */
  id: string;
  name: string;
  seal: string;
  avatar: string | null;
  local: LocalGhost | null;
  /** Where her daemon listens. `null` while she is not running. */
  url: string | null;
  watch: LinkState | null;
  talk: LinkState | null;
  status: Status | null;
  agenda: Agenda | null;
  identity: Identity | null;
  people: PersonRow[];
  /** Her conversation with whoever is at this screen, oldest first. */
  entries: Entry[];
  loaded: boolean;
  /** When she was last seen typing to you. */
  typingAt: number;
};

export type View = "chat" | "birth" | "settings" | "certificate";

export type Settings = {
  /** The name she knows you by. */
  you: string;
  /** The daemon to talk to when there is no keeper. */
  remote: string;
  token: string;
};

export type State = {
  settings: Settings;
  /** This machine, as the keeper sees it. `null` where there is no keeper. */
  local: Local | null;
  sessions: Session[];
  active: string | null;
  view: View;
  sidebar: boolean;
  panel: boolean;
  /** Per ghost, the seq of the newest thing she said that you have seen. */
  seen: Record<string, number>;
  /** Per ghost, something the keeper is in the middle of doing for her. */
  busy: Record<string, string>;
  /** The last thing that went wrong, in words. */
  trouble: string | null;
};

const THREAD_KINDS = new Set([
  "heard", "read", "said", "reacted", "decided", "felt", "noted", "intended", "settled", "left", "canon",
]);

function stored<T>(key: string, fallback: T): T {
  try {
    const saved = localStorage.getItem(key);
    return saved ? { ...fallback, ...(JSON.parse(saved) as Partial<T>) } : fallback;
  } catch {
    return fallback;
  }
}

function store(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Private windows have no storage; it lasts for this session.
  }
}

let state: State = {
  settings: stored<Settings>("ghost.console.settings", { you: "you", remote: DEFAULT_URL, token: "" }),
  local: null,
  sessions: [],
  active: stored<{ id: string | null }>("ghost.console.active", { id: null }).id,
  view: "chat",
  sidebar: true,
  panel: stored<{ open: boolean }>("ghost.console.panel", { open: true }).open,
  seen: stored<Record<string, number>>("ghost.console.seen", {}),
  busy: {},
  trouble: null,
};

const listeners = new Set<() => void>();

function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  for (const listener of listeners) listener();
}

function patch(id: string, change: Partial<Session>) {
  set({ sessions: state.sessions.map((s) => (s.id === id ? { ...s, ...change } : s)) });
}

export function useStore<T>(select: (state: State) => T): T {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => select(state),
  );
}

export function getState(): State {
  return state;
}

/** The ghost being looked at. */
export function useActive(): Session | null {
  return useStore((s) => s.sessions.find((session) => session.id === s.active) ?? null);
}

function placeId(): string {
  return state.settings.you.trim().toLowerCase() || "you";
}

/** The key of the private place between a ghost and whoever is at this screen. */
export function mine(): string {
  return `${CHANNEL}:${placeId()}`;
}

/** How many things she has said that you have not looked at. */
export function unread(session: Session, seen: Record<string, number>): number {
  const upto = seen[session.id] ?? 0;
  return session.entries.filter((e) => e.event.kind === "said" && e.seq > upto).length;
}

// --- wires: the two links to one ghost's daemon -------------------------------

type Wire = { url: string; watch: Link; talk: Link };
const wires = new Map<string, Wire>();
const pending = new Map<string, ReturnType<typeof setTimeout>>();

/** Run `work` once, shortly, however many times it is asked for meanwhile. */
function soon(name: string, work: () => void, wait = 120) {
  if (pending.has(name)) return;
  pending.set(
    name,
    setTimeout(() => {
      pending.delete(name);
      work();
    }, wait),
  );
}

async function ask<T>(id: string, method: string, params?: unknown): Promise<T | null> {
  const wire = wires.get(id);
  if (!wire?.watch.isOpen) return null;
  try {
    return await wire.watch.call<T>(method, params);
  } catch {
    return null;
  }
}

const refresh = {
  async status(id: string) {
    const status = await ask<Status>(id, "ghost.status");
    if (status) patch(id, { status });
  },
  async agenda(id: string) {
    const agenda = await ask<Agenda>(id, "mind.agenda");
    if (agenda) patch(id, { agenda });
  },
  async people(id: string) {
    const people = await ask<PersonRow[]>(id, "memory.people");
    if (people) patch(id, { people });
  },
  async identity(id: string) {
    const identity = await ask<Identity>(id, "ghost.identity");
    if (identity) patch(id, { identity });
  },
  async thread(id: string) {
    const entries = await ask<Entry[]>(id, "journal.place", { place: mine(), limit: 500 });
    if (!entries) return;
    patch(id, { entries, loaded: true });
    // Looking at her conversation is what marks it seen.
    if (state.active === id && state.view === "chat" && document.visibilityState === "visible") markSeen(id);
  },
};

function connect(session: Session, url: string) {
  const { id } = session;
  const token = state.settings.token.trim();
  const auth = token ? { token } : {};
  const watch = new Link(url, { protocol: PROTOCOL, role: "console", ...auth });
  const talk = new Link(url, {
    protocol: PROTOCOL,
    role: "presence",
    ...auth,
    presence: { channel: CHANNEL, can: ["send", "typing", "react"] },
  });

  watch.onState = (watching) => {
    patch(id, { watch: watching });
    if (watching.is !== "open") return;
    if (!session.local) patch(id, { name: watching.welcome.ghost.name, seal: watching.welcome.ghost.seal });
    void Promise.all([refresh.status(id), refresh.agenda(id), refresh.identity(id), refresh.people(id)]);
    void refresh.thread(id).then(() => ask(id, "journal.subscribe"));
  };
  watch.onNotify = (method, params) => {
    if (method !== "journal.entry") return;
    const kind = (params as Entry).event.kind;
    if (THREAD_KINDS.has(kind)) soon(`thread:${id}`, () => void refresh.thread(id));
    if (kind === "woke" || kind === "slept" || kind === "planned" || kind === "lived") {
      soon(`status:${id}`, () => void refresh.status(id));
    }
    if (kind === "felt" || kind === "noted" || kind === "heard") soon(`people:${id}`, () => void refresh.people(id), 400);
    soon(`agenda:${id}`, () => void refresh.agenda(id));
  };

  talk.onState = (talking) => patch(id, { talk: talking });
  talk.onNotify = (method) => {
    // Another window is talking to her now. Stop, or the two would take the
    // conversation from each other forever.
    if (method === "presence.displaced") {
      talk.stop();
      patch(id, { talk: { is: "refused", why: "another window is talking to her now", code: 0 } });
    }
  };
  // She did something on the console channel. Her journal will show it; all
  // this side has to do is acknowledge, and notice when she is typing.
  talk.onRequest = async (method, params) => {
    if (method !== "presence.act") throw new Error(`unknown method ${method}`);
    const action = params as Action;
    if (action.do === "typing") {
      patch(id, { typingAt: Date.now() });
      return {};
    }
    if (action.do === "send") {
      patch(id, { typingAt: 0 });
      return { id: `c-${Date.now()}-${Math.random().toString(36).slice(2, 8)}` };
    }
    if (action.do === "react") return {};
    throw new Error(`the console cannot ${action.do}`);
  };

  wires.set(id, { url, watch, talk });
  watch.start();
  talk.start();
}

function disconnect(id: string) {
  const wire = wires.get(id);
  if (!wire) return;
  wire.watch.stop();
  wire.talk.stop();
  wires.delete(id);
}

/** Make the sessions and their wires match who lives where. */
function sync() {
  const before = new Map(state.sessions.map((s) => [s.id, s]));
  const fresh = (id: string, name: string, seal: string): Session =>
    before.get(id) ?? {
      id,
      name,
      seal,
      avatar: null,
      local: null,
      url: null,
      watch: null,
      talk: null,
      status: null,
      agenda: null,
      identity: null,
      people: [],
      entries: [],
      loaded: false,
      typingAt: 0,
    };

  const sessions: Session[] = hasKeeper
    ? (state.local?.ghosts ?? []).map((ghost) => ({
        ...fresh(ghost.path, ghost.name, ghost.seal),
        name: ghost.name,
        seal: ghost.seal,
        avatar: ghost.avatar,
        local: ghost,
        url: ghost.url,
      }))
    : [{ ...fresh(REMOTE, "Ghost", ""), url: state.settings.remote }];

  for (const id of [...wires.keys()]) {
    const session = sessions.find((s) => s.id === id);
    if (!session || session.url !== wires.get(id)!.url) disconnect(id);
  }
  // Someone who is not running has no connection state to show.
  for (const session of sessions) {
    if (!session.url) Object.assign(session, { watch: null, talk: null, agenda: null, typingAt: 0 });
  }
  const active = sessions.some((s) => s.id === state.active) ? state.active : (sessions[0]?.id ?? null);
  set({ sessions, active });
  for (const session of sessions) {
    if (session.url && !wires.has(session.id)) connect(session, session.url);
  }
}

/** Start the console: learn who lives here, connect to whoever is awake. */
export function start() {
  if (hasKeeper) {
    void refreshLocal();
    // A daemon can stop on its own; ask the keeper now and then.
    setInterval(() => void refreshLocal(), 20000);
  } else {
    sync();
  }
  // What she is about to do changes with the clock, not only with events.
  setInterval(() => {
    for (const id of wires.keys()) void refresh.agenda(id);
  }, 2500);
  setInterval(() => {
    for (const id of wires.keys()) void refresh.status(id);
  }, 20000);
  window.addEventListener("focus", () => {
    if (state.active && state.view === "chat") markSeen(state.active);
  });
}

// While developing, a hot-replaced copy of this module must let go of its
// connections, or it would go on holding the channel against its successor.
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    for (const id of [...wires.keys()]) disconnect(id);
  });
}

// --- looking and talking ---------------------------------------------------------

function markSeen(id: string) {
  const session = state.sessions.find((s) => s.id === id);
  const last = session?.entries[session.entries.length - 1]?.seq ?? 0;
  if (last <= (state.seen[id] ?? 0)) return;
  const seen = { ...state.seen, [id]: last };
  store("ghost.console.seen", seen);
  set({ seen });
}

export function select(id: string) {
  store("ghost.console.active", { id });
  set({ active: id, view: "chat", trouble: null });
  markSeen(id);
}

export function show(view: View) {
  set({ view, trouble: null });
}

export function toggleSidebar() {
  set({ sidebar: !state.sidebar });
}

export function togglePanel() {
  store("ghost.console.panel", { open: !state.panel });
  set({ panel: !state.panel });
}

/** Say something to the ghost being looked at, as whoever is at this screen. */
export function say(text: string): boolean {
  const body = text.trim();
  const wire = state.active ? wires.get(state.active) : undefined;
  if (!body || !wire?.talk.isOpen) return false;
  wire.talk.notify("presence.inbound", {
    place: { id: placeId(), kind: "direct" },
    from: { id: placeId(), name: state.settings.you.trim() || "you" },
    id: `u-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    text: body,
    addressed: true,
  });
  return true;
}

/** Let her see that someone is still writing, so she waits before looking. */
export function typingToHer() {
  const wire = state.active ? wires.get(state.active) : undefined;
  if (wire?.talk.isOpen) wire.talk.notify("presence.typing", { place: placeId() });
}

/** Take the conversation back from whichever window has it. */
export function talkFromHere(id: string) {
  const wire = wires.get(id);
  if (!wire) return;
  patch(id, { talk: { is: "connecting" } });
  wire.talk.start();
}

export function saveSettings(settings: Settings) {
  store("ghost.console.settings", settings);
  set({ settings });
  // A new name or address means every conversation is a different one.
  for (const id of [...wires.keys()]) disconnect(id);
  set({ sessions: state.sessions.map((s) => ({ ...s, entries: [], loaded: false })) });
  sync();
}

// --- this machine ------------------------------------------------------------------

export async function refreshLocal(): Promise<void> {
  if (!hasKeeper) return;
  try {
    set({ local: await keep<Local>("state") });
    sync();
  } catch (why) {
    set({ trouble: (why as Error).message });
  }
}

/** Ask the keeper to do something for a ghost, and take in what changed. */
async function tend(id: string, doing: string, cmd: string, args: Record<string, unknown>): Promise<boolean> {
  set({ busy: { ...state.busy, [id]: doing }, trouble: null });
  let ok = true;
  try {
    set({ local: await keep<Local>(cmd, args) });
    sync();
  } catch (why) {
    ok = false;
    // Something may have half happened (stopped, but not started again), so
    // look at how things are now instead of assuming nothing changed.
    try {
      set({ local: await keep<Local>("state") });
      sync();
    } catch {
      // The keeper itself is unreachable; the message below says why.
    }
    set({ trouble: (why as Error).message });
  }
  const { [id]: _done, ...busy } = state.busy;
  set({ busy });
  return ok;
}

export const wake = (id: string) => tend(id, "Waking", "wake", { path: id });
export const rest = (id: string) => tend(id, "Stopping", "rest", { path: id });
export const chooseModel = (id: string, model: string) => tend(id, "Switching", "set_model", { path: id, model });
export const setAvatar = (id: string, avatar: string) => tend(id, "Saving", "set_avatar", { path: id, avatar });
export const saveModel = (model: Model) => tend("models", "Saving", "save_model", { model });
export const deleteModel = (id: string) => tend("models", "Removing", "delete_model", { id });
export const setTempo = (tempo: number) => tend("models", "Saving", "set_tempo", { tempo });

/** Give birth, then wake her if there is something for her to think with. */
export async function giveBirth(certificate: string, avatar: string | null): Promise<void> {
  const born = await keep<LocalGhost>("birth", { certificate, avatar });
  await refreshLocal();
  select(born.path);
  if ((state.local?.models.length ?? 0) > 0) await wake(born.path);
}
