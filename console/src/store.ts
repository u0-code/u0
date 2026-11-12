// Everything the console knows, and the connections that feed it.
//
// The console is two clients of one daemon. As a *console* it watches: her
// status, the places she is in, the people she knows, what she is about to
// do, and her journal as it is written. As a *presence* it carries one
// channel, `console`, on which whoever is at this screen can write to her
// like anyone else. It cannot speak for her and has no way to change her;
// the protocol offers none.
//
// Separately, in the desktop app, it asks the keeper about this machine:
// which ghosts live here, what her body runs on, whether it is running.

import { useSyncExternalStore } from "react";
import { dayKey } from "./format";
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
import { hasKeeper, keep, type Body, type Local, type LocalGhost } from "./shell";

export const CHANNEL = "console";

/** What the record on the right is showing. */
export type Scope =
  /** One day of her life. `null` is today. */
  | { is: "day"; day: string | null }
  /** One conversation, across all its days. */
  | { is: "place"; key: string };

/** A document laid over the window. */
export type Sheet = null | "certificate" | "birth" | "body" | "ghosts";

export type Settings = {
  url: string;
  token: string;
  you: string;
  /** Whether the window is lit by her day or by this computer's setting. */
  light: "hers" | "system";
};

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
  /** Entries of the conversation being looked at. */
  thread: { key: string; entries: Entry[]; loaded: boolean };
  /** Place key to the time she was last seen typing there. */
  typing: Record<string, number>;
  scope: Scope;
  sheet: Sheet;
  /** This machine, as the keeper sees it. `null` where there is no keeper. */
  local: Local | null;
};

const JOURNAL_KEPT = 3000;
const THREAD_KINDS = new Set([
  "heard", "read", "said", "reacted", "decided", "felt", "noted", "intended", "settled", "left", "canon",
]);

function loadSettings(): Settings {
  const fallback: Settings = { url: DEFAULT_URL, token: "", you: "you", light: "hers" };
  try {
    const saved = localStorage.getItem("ghost.console.settings");
    return saved ? { ...fallback, ...(JSON.parse(saved) as Partial<Settings>) } : fallback;
  } catch {
    return fallback;
  }
}

function placeId(settings: Settings): string {
  return settings.you.trim().toLowerCase() || "you";
}

/** The key of the private place between her and whoever is at this screen. */
export function talkKey(settings: Settings): string {
  return `${CHANNEL}:${placeId(settings)}`;
}

let state: State = {
  settings: loadSettings(),
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
  thread: { key: "", entries: [], loaded: false },
  typing: {},
  scope: { is: "day", day: null },
  sheet: null,
  local: null,
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

// --- the daemon -------------------------------------------------------------

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
  if (state.scope.is !== "place") return;
  const key = state.scope.key;
  const entries = await ask<Entry[]>("journal.place", { place: key, limit: 600 });
  // The reader may have moved on while this was in flight.
  if (entries && state.scope.is === "place" && state.scope.key === key) {
    set({ thread: { key, entries, loaded: true } });
  }
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
  const journal = await ask<Entry[]>("journal.read", { from: 1 - JOURNAL_KEPT, limit: JOURNAL_KEPT });
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
  set({ watch: { is: "connecting" }, talk: { is: "connecting" } });

  watch = new Link(url, { protocol: PROTOCOL, role: "console", ...auth });
  watch.onState = (watching) => {
    const lost = watching.is !== "open" && state.watch.is === "open";
    set({ watch: watching });
    if (watching.is === "open") {
      set({ ghost: watching.welcome.ghost });
      void onWatching();
    }
    // The daemon appearing or vanishing changes what this machine is running.
    if (watching.is === "open" || lost) void refreshLocal();
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
  talk.onNotify = (method) => {
    // Another window is talking to her now. Stop, or the two would take the
    // conversation from each other forever.
    if (method === "presence.displaced") {
      talk?.stop();
      set({ talk: { is: "refused", why: "another window is talking to her now", code: 0 } });
    }
  };
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

// While developing, a hot-replaced copy of this module must let go of its
// connections, or it would go on holding the channel against its successor.
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    watch?.stop();
    talk?.stop();
    if (poll) clearInterval(poll);
  });
}

// --- looking ----------------------------------------------------------------

export function look(scope: Scope) {
  if (scope.is === "place" && (state.scope.is !== "place" || state.scope.key !== scope.key)) {
    set({ scope, sheet: null, thread: { key: scope.key, entries: [], loaded: false } });
    void refreshThread();
  } else {
    set({ scope, sheet: null });
  }
}

export function open(sheet: Sheet) {
  set({ sheet });
  if (sheet === "body" || sheet === "ghosts") void refreshLocal();
}

/** The calendar day, in her timezone, that `scope` shows. */
export function dayOf(scope: Scope, timeZone: string | undefined): string {
  return scope.is === "day" && scope.day ? scope.day : dayKey(new Date().toISOString(), timeZone);
}

/** Take the conversation back from whichever window has it. */
export function talkFromHere() {
  if (!talk) return;
  set({ talk: { is: "connecting" } });
  talk.start();
}

/** Say something to her, as whoever is at this screen. */
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

/** Let her see that someone is still writing, so she waits before looking. */
export function typingToHer() {
  if (talk?.isOpen) talk.notify("presence.typing", { place: placeId(state.settings) });
}

export function saveSettings(settings: Settings) {
  try {
    localStorage.setItem("ghost.console.settings", JSON.stringify(settings));
  } catch {
    // Private windows have no storage; the settings last for this session.
  }
  const reconnect =
    settings.url !== state.settings.url ||
    settings.token !== state.settings.token ||
    settings.you !== state.settings.you;
  set({ settings });
  if (reconnect) connect();
}

// --- this machine -----------------------------------------------------------

export async function refreshLocal(): Promise<Local | null> {
  if (!hasKeeper) return null;
  try {
    const local = await keep<Local>("state");
    set({ local });
    return local;
  } catch {
    return null;
  }
}

/** Give birth. Throws with the reason if she could not be born. */
export async function giveBirth(certificate: string): Promise<LocalGhost> {
  const born = await keep<LocalGhost>("birth", { certificate });
  await refreshLocal();
  return born;
}

export async function setBody(body: Body): Promise<void> {
  await keep("set_body", { body });
  await refreshLocal();
}

/** Start her body. The daemon is its own process; the links find it. */
export async function wake(path: string): Promise<void> {
  await keep("wake", { path });
  await refreshLocal();
  connect();
}

export async function rest(): Promise<void> {
  await keep("rest");
  await refreshLocal();
}
