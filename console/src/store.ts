// Everything the console knows, and the two connections that feed it.
//
// The console is two clients of the same daemon. As a *console* it watches:
// status, places, people, agenda, and her journal as it is written. As a
// *presence* it carries one channel, `console`, on which the person at this
// screen can talk to her like anyone else. It cannot speak for her and has no
// way to change her; the protocol offers none.

import { useSyncExternalStore } from "react";
import { Link, type LinkState } from "./protocol/link";
import {
  DEFAULT_URL,
  PROTOCOL,
  type Action,
  type Agenda,
  type Entry,
  type Fact,
  type GhostCard,
  type Identity,
  type PersonRow,
  type PlaceRow,
  type Status,
} from "./protocol/types";

export const CHANNEL = "console";

export type View = { is: "thread"; key: string } | { is: "journal" } | { is: "people" } | { is: "life" };

export type Settings = { url: string; token: string; you: string };

export type State = {
  settings: Settings;
  watch: LinkState;
  talk: LinkState;
  ghost: GhostCard | null;
  identity: Identity | null;
  /** What is true about her life, oldest first. */
  canon: Fact[];
  status: Status | null;
  agenda: Agenda | null;
  places: PlaceRow[];
  people: PersonRow[];
  /** The tail of her journal, oldest first. */
  journal: Entry[];
  /** Entries of the thread being looked at. */
  thread: { key: string; entries: Entry[]; loaded: boolean };
  /** Place key to the time she was last seen typing there. */
  typing: Record<string, number>;
  view: View;
  sidebar: boolean;
  inspector: boolean;
};

const JOURNAL_KEPT = 1500;
const THREAD_KINDS = new Set([
  "heard", "read", "said", "reacted", "decided", "felt", "noted", "intended", "settled", "left", "canon",
]);

function loadSettings(): Settings {
  const fallback: Settings = { url: DEFAULT_URL, token: "", you: "you" };
  try {
    const saved = localStorage.getItem("ghost.console.settings");
    return saved ? { ...fallback, ...(JSON.parse(saved) as Partial<Settings>) } : fallback;
  } catch {
    return fallback;
  }
}

/** The key of the private place between her and the person at this screen. */
export function talkKey(settings: Settings): string {
  return `${CHANNEL}:${placeId(settings)}`;
}

function placeId(settings: Settings): string {
  return settings.you.trim().toLowerCase() || "you";
}

const initialSettings = loadSettings();

let state: State = {
  settings: initialSettings,
  watch: { is: "connecting" },
  talk: { is: "connecting" },
  ghost: null,
  identity: null,
  canon: [],
  status: null,
  agenda: null,
  places: [],
  people: [],
  journal: [],
  thread: { key: talkKey(initialSettings), entries: [], loaded: false },
  typing: {},
  view: { is: "thread", key: talkKey(initialSettings) },
  sidebar: true,
  inspector: false,
};

const listeners = new Set<() => void>();

function set(patch: Partial<State>) {
  state = { ...state, ...patch };
  for (const listener of listeners) listener();
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

// --- connections -----------------------------------------------------------

let watch: Link | null = null;
let talk: Link | null = null;
let poll: ReturnType<typeof setInterval> | null = null;
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

async function ask<T>(method: string, params?: unknown): Promise<T | null> {
  if (!watch?.isOpen) return null;
  try {
    return await watch.call<T>(method, params);
  } catch {
    return null;
  }
}

async function refreshStatus() {
  const status = await ask<Status>("ghost.status");
  if (status) set({ status });
}

async function refreshAgenda() {
  const agenda = await ask<Agenda>("mind.agenda");
  if (agenda) set({ agenda });
}

async function refreshPlaces() {
  const places = await ask<PlaceRow[]>("memory.places");
  if (places) set({ places });
}

async function refreshPeople() {
  const people = await ask<PersonRow[]>("memory.people");
  if (people) set({ people });
}

async function refreshLife() {
  const [identity, canon] = await Promise.all([ask<Identity>("ghost.identity"), ask<Fact[]>("memory.canon")]);
  if (identity) set({ identity });
  if (canon) set({ canon });
}

async function refreshThread() {
  const key = state.thread.key;
  const entries = await ask<Entry[]>("journal.place", { place: key, limit: 400 });
  // The view may have moved on while this was in flight.
  if (entries && state.thread.key === key) set({ thread: { key, entries, loaded: true } });
}

function onEntry(entry: Entry) {
  const journal = [...state.journal, entry];
  set({ journal: journal.length > JOURNAL_KEPT ? journal.slice(-JOURNAL_KEPT) : journal });
  const kind = entry.event.kind;
  if (THREAD_KINDS.has(kind)) soon("thread", refreshThread);
  if (kind === "heard" || kind === "said" || kind === "read") soon("places", refreshPlaces);
  if (kind === "heard" || kind === "felt" || kind === "noted" || kind === "left") soon("people", refreshPeople, 400);
  if (kind === "woke" || kind === "slept" || kind === "planned" || kind === "lived") soon("status", refreshStatus);
  if (kind === "canon") soon("life", refreshLife, 400);
  soon("agenda", refreshAgenda);
}

async function onWatching() {
  await Promise.all([refreshStatus(), refreshPlaces(), refreshPeople(), refreshAgenda(), refreshLife()]);
  const journal = await ask<Entry[]>("journal.read", { from: -299, limit: 300 });
  if (journal) set({ journal });
  await ask("journal.subscribe");
  await refreshThread();
}

/** She did something on the console channel. Her journal will show it; all
 *  this side has to do is acknowledge, and notice when she is typing. */
async function carryOut(action: Action): Promise<unknown> {
  const key = "place" in action ? `${CHANNEL}:${action.place}` : "";
  switch (action.do) {
    case "typing":
      set({ typing: { ...state.typing, [key]: Date.now() } });
      return {};
    case "send": {
      const { [key]: _done, ...typing } = state.typing;
      set({ typing });
      return { id: `c-${Date.now()}-${Math.random().toString(36).slice(2, 8)}` };
    }
    case "react":
      return {};
    default:
      throw new Error(`the console cannot ${action.do}`);
  }
}

export function connect() {
  watch?.stop();
  talk?.stop();
  if (poll) clearInterval(poll);
  const { url, token } = state.settings;
  const auth = token.trim() ? { token: token.trim() } : {};
  set({
    watch: { is: "connecting" },
    talk: { is: "connecting" },
    thread: { ...state.thread, loaded: false },
  });

  watch = new Link(url, { protocol: PROTOCOL, role: "console", ...auth });
  watch.onState = (watching) => {
    set({ watch: watching });
    if (watching.is === "open") {
      set({ ghost: watching.welcome.ghost });
      void onWatching();
    }
  };
  watch.onNotify = (method, params) => {
    if (method === "journal.entry") onEntry(params as Entry);
  };
  watch.start();

  talk = new Link(url, {
    protocol: PROTOCOL,
    role: "presence",
    ...auth,
    presence: { channel: CHANNEL, can: ["send", "typing", "react"] },
  });
  talk.onState = (talking) => set({ talk: talking });
  talk.onRequest = async (method, params) => {
    if (method !== "presence.act") throw new Error(`unknown method ${method}`);
    return carryOut(params as Action);
  };
  talk.start();

  // What she is about to do changes with the clock, not only with events.
  poll = setInterval(() => {
    void refreshAgenda();
    soon("status", refreshStatus, 15000);
  }, 2500);
}

// --- actions ---------------------------------------------------------------

export function show(view: View) {
  if (view.is === "thread" && view.key !== state.thread.key) {
    set({ view, thread: { key: view.key, entries: [], loaded: false } });
    void refreshThread();
  } else {
    set({ view });
  }
}

export function toggleSidebar() {
  set({ sidebar: !state.sidebar });
}

export function toggleInspector() {
  set({ inspector: !state.inspector });
}

/** Say something to her, as the person at this screen. */
export function say(text: string): boolean {
  const body = text.trim();
  if (!body || !talk?.isOpen) return false;
  const you = state.settings.you.trim() || "you";
  talk.notify("presence.inbound", {
    place: { id: placeId(state.settings), kind: "direct" },
    from: { id: placeId(state.settings), name: you },
    id: `u-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    text: body,
    addressed: true,
  });
  return true;
}

/** She is waiting on nobody, but it is polite to let her know. */
export function typingToHer() {
  if (talk?.isOpen) talk.notify("presence.typing", { place: placeId(state.settings) });
}

export function saveSettings(settings: Settings) {
  try {
    localStorage.setItem("ghost.console.settings", JSON.stringify(settings));
  } catch {
    // Private windows have no storage; the settings last for this session.
  }
  const moved = talkKey(settings) !== talkKey(state.settings);
  set({ settings });
  if (moved) show({ is: "thread", key: talkKey(settings) });
  connect();
}
