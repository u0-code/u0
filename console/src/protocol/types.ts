// The Ghost Protocol as the console sees it. Mirrors crates/ghost-protocol and
// the serialized shapes of ghost-core. The console payloads are not frozen
// yet, so everything optional is treated as optional.

export const PROTOCOL = 1;
export const DEFAULT_URL = "ws://127.0.0.1:7437";

export type Frame = {
  jsonrpc: "2.0";
  id?: number;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: { code: number; message: string };
};

export type Actor = { id: string; name: string; handle?: string; automated?: boolean };
export type PlaceKind = "direct" | "group" | "public";
export type Place = { id: string; kind: PlaceKind; name?: string };
export type Capability = "send" | "typing" | "react" | "read" | "post" | "feed";

export type Hello = {
  protocol: number;
  role: "presence" | "console";
  token?: string;
  presence?: { channel: string; me?: Actor; can: Capability[] };
};

export type GhostCard = { name: string; pronouns: string; city: string; country: string; seal: string };
export type Welcome = { protocol: number; ghost: GhostCard };

/** What the daemon asks a presence to do. */
export type Action =
  | { do: "send"; place: string; text: string; reply_to?: string }
  | { do: "typing"; place: string }
  | { do: "react"; place: string; to: string; emoji: string }
  | { do: "read"; place: string; upto?: string }
  | { do: "post"; text: string }
  | { do: "fetch"; feed: string; limit: number };

export type Block = { from: string; to: string; what: string; reach: Reach };
export type Reach = "free" | "glance" | "busy" | "away";

/** One thing in her journal. `data` depends on `kind`. */
export type Event =
  | { kind: "born"; data: { name: string; seal: string } }
  | { kind: "dormant"; data: { since: string } }
  | { kind: "woke" }
  | { kind: "slept" }
  | { kind: "planned"; data: { day: string; weather?: string; blocks: Block[] } }
  | { kind: "lived"; data: { text: string; block?: number } }
  | {
      kind: "heard";
      data: { channel: string; place: Place; from: Actor; id: string; text: string; reply_to?: string; addressed?: boolean };
    }
  | { kind: "read"; data: { channel: string; place: string; upto: number } }
  | { kind: "said"; data: { channel: string; place: string; id?: string; text: string; reply_to?: string } }
  | { kind: "reacted"; data: { channel: string; place: string; to: string; emoji: string } }
  | { kind: "posted"; data: { channel: string; id?: string; text: string } }
  | { kind: "saw"; data: { channel: string; text: string } }
  | { kind: "decided"; data: { about: string; decision: string; reason: string } }
  | { kind: "felt"; data: { toward: string; delta: number; why: string } }
  | { kind: "noted"; data: { about: string; text: string } }
  | { kind: "intended"; data: { due: string; what: string; about?: string; place?: string } }
  | { kind: "settled"; data: { intention: number; outcome: string } }
  | { kind: "left"; data: { person: string; why: string } }
  | { kind: "reflected"; data: { text: string } }
  | { kind: "canon"; data: { fact: string; source: string } };

export type Entry = { seq: number; at: string; event: Event; prev_hash: string; hash: string };
export type Kind = Event["kind"];

export type Doing = { index: number; what: string; reach: Reach; from: string; to: string };

export type Status = {
  name: string;
  pronouns: string;
  city: string;
  country: string;
  seal: string;
  timezone: string;
  local_time: string;
  moment: { phase: "awake" | "asleep"; doing: Doing | null; reach: Reach; until: string };
  today: { seq: number; day: string; weather: string | null; blocks: Doing[] } | null;
  journal: number;
  channels: { channel: string; me: Actor | null; can: Capability[] }[];
  tempo: number;
  brains: { think: string; tick: string };
  calls_this_hour: number;
};

export type Agenda = {
  attend: { place: string; due: string; addressed: boolean }[];
  thinking: string[];
  speaking: string[];
  life: string | null;
  intention: string | null;
  phone: string | null;
};

export type PlaceState = {
  key: string;
  channel: string;
  id: string;
  kind: PlaceKind;
  name: string | null;
  read_upto: number;
  last_in_seq: number;
  last_in_at: string | null;
  last_out_seq: number;
  last_out_at: string | null;
  unread: number;
  unread_addressed: number;
};
export type PlaceRow = { place: PlaceState; with: string | null; online: boolean };

/** Her birth certificate, as written once and sealed. */
export type Seed = {
  name: string;
  pronouns: string;
  born: string;
  home: { city: string; country: string; timezone: string };
  rhythm: { wake: string; sleep: string };
  temperament: string;
  voice: string;
  origin: string;
  values: string[];
  cares_about: string[];
  cannot_stand: string[];
};
export type Identity = { seed: Seed; seal: string };
export type Fact = { seq: number; at: string; fact: string; source: string };

export type Stage = "gone" | "estranged" | "cooling" | "stranger" | "acquaintance" | "friend" | "close";
export type PersonRow = {
  person: {
    key: string;
    channel: string;
    id: string;
    name: string;
    handle: string | null;
    automated: boolean;
    first_at: string;
    last_at: string;
    heard: number;
    warmth: number;
    gone: boolean;
    direct: string | null;
    active_days: number;
  };
  standing: { stage: Stage; warmth: number; known_days: number; active_days: number };
  notes: { seq: number; at: string; text: string }[];
};
