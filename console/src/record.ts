// Her journal, set as a record.
//
// The journal is a list of events. A record is what a reader gets: the day
// broken where she woke and slept, conversations gathered into episodes
// under who they were with, what people wrote quoted, what she said in full,
// and beside both the trace of her mind. Two things a chat window hides are
// kept on purpose: how long a message waited before she read it, and the
// moments she chose to do nothing.

import { clock, day, dayKey, span } from "./format";
import type { Entry, PersonRow, PlaceRow } from "./protocol/types";

export type Row = { key: string; at: string } & (
  | { is: "break"; label: string; night?: boolean }
  /** A conversation begins, or she turns to another one. */
  | { is: "episode"; place: string; title: string; meta: string }
  | { is: "heard"; who: string | null; text: string; aside: boolean }
  | { is: "said"; text: string }
  /** What went through her mind. `did` is what came of it. */
  | { is: "thought"; text: string; did?: string }
  /** An act of will: saying nothing, holding back, leaving. */
  | { is: "will"; verb: string; text: string }
  /** A small fact in the margin of the day. */
  | { is: "mark"; label: string; text: string; delta?: number }
  | { is: "lived"; text: string; title?: string }
  | { is: "reflected"; text: string }
);

export type Context = {
  timeZone: string | undefined;
  places: PlaceRow[];
  people: PersonRow[];
  /** The key of the place between her and whoever is at this screen. */
  mine: string;
  /** One place only: no episode headings, day headings instead. */
  single: boolean;
};

/** Which place an entry belongs to, if it belongs to one. */
export function placeOf(entry: Entry): string | null {
  const e = entry.event;
  switch (e.kind) {
    case "heard":
      return `${e.data.channel}:${e.data.place.id}`;
    case "said":
    case "read":
    case "reacted":
      return `${e.data.channel}:${e.data.place}`;
    case "decided":
      return e.data.about;
    default:
      return null;
  }
}

export function placeTitle(key: string, ctx: Pick<Context, "places" | "mine">): { title: string; meta: string } {
  const row = ctx.places.find((p) => p.place.key === key);
  const channel = key.split(":")[0] ?? "";
  if (key === ctx.mine) return { title: "with you", meta: "this window" };
  if (!row) return { title: key, meta: channel };
  if (row.place.kind === "direct") return { title: `with ${row.with ?? row.place.id}`, meta: channel };
  if (row.place.kind === "group") return { title: `in ${row.place.name ?? "a group"}`, meta: `${channel} · group` };
  return { title: "in public", meta: `${channel} · anyone can read` };
}

export function compose(entries: Entry[], ctx: Context): Row[] {
  const rows: Row[] = [];
  const name = (key: string) => ctx.people.find((p) => p.person.key === key)?.person.name;
  let place: string | null = null;
  let speaker = "";
  let today = "";
  let unreadSince: string | null = null;
  let plan: string[] = [];

  for (const entry of entries) {
    const key = `e${entry.seq}`;
    const at = entry.at;
    const e = entry.event;

    if (ctx.single) {
      const dk = dayKey(at, ctx.timeZone);
      if (dk !== today) {
        today = dk;
        speaker = "";
        rows.push({ is: "break", key: `d${dk}`, at, label: day(at, ctx.timeZone) });
      }
    } else {
      // Turning to a different conversation starts a new episode.
      const here = placeOf(entry);
      if (here !== null && here !== place) {
        place = here;
        speaker = "";
        unreadSince = null;
        rows.push({ is: "episode", key: `p${entry.seq}`, at, place: here, ...placeTitle(here, ctx) });
      }
    }

    switch (e.kind) {
      case "heard": {
        unreadSince ??= at;
        const shared = e.data.place.kind !== "direct";
        // Name the speaker when the voice changes, so their words are never
        // mistaken for hers.
        const theirs = `${e.data.channel}:${e.data.place.id}` === ctx.mine ? "you" : e.data.from.name;
        const who = e.data.from.id !== speaker ? theirs : null;
        speaker = e.data.from.id;
        rows.push({ is: "heard", key, at, who, text: e.data.text, aside: shared && !e.data.addressed });
        continue;
      }
      case "said":
        speaker = "";
        rows.push({ is: "said", key, at, text: e.data.text });
        continue;
      case "read":
        if (unreadSince) {
          rows.push({ is: "mark", key, at, label: "read", text: `after ${span(unreadSince, at)}` });
          unreadSince = null;
        }
        continue;
      case "decided": {
        speaker = "";
        const { decision, reason } = e.data;
        if (decision.startsWith("stayed quiet")) {
          const why = decision.slice("stayed quiet".length).replace(/^[:;\s]+/, "");
          rows.push({ is: "will", key, at, verb: "said nothing", text: reason || why });
        } else if (decision === "held back") {
          rows.push({ is: "will", key, at, verb: "held back", text: "They wrote again before she sent what she had typed." });
        } else if (decision === "stayed out of it") {
          rows.push({ is: "will", key, at, verb: "stayed out of it", text: reason });
        } else if (decision === "could not send") {
          rows.push({ is: "mark", key, at, label: "not sent", text: reason });
        } else if (decision === "nothing") {
          if (reason) rows.push({ is: "will", key, at, verb: "let it pass", text: reason });
        } else if (reason) {
          rows.push({ is: "thought", key, at, text: reason, did: decision });
        }
        continue;
      }
      case "reacted":
        rows.push({ is: "mark", key, at, label: "reacted", text: e.data.emoji });
        continue;
      case "felt": {
        const toward = place === null || ctx.single ? name(e.data.toward) : undefined;
        rows.push({
          is: "mark",
          key,
          at,
          label: toward && !ctx.single ? `felt, toward ${toward}` : "felt",
          text: e.data.why,
          delta: e.data.delta,
        });
        continue;
      }
      case "noted": {
        const about = place === null ? name(e.data.about) : undefined;
        rows.push({ is: "mark", key, at, label: about ? `noted, of ${about}` : "noted", text: e.data.text });
        continue;
      }
      case "intended":
        rows.push({
          is: "mark",
          key,
          at,
          label: "means to",
          text: `${e.data.what}, by ${clock(e.data.due, ctx.timeZone)} on ${day(e.data.due, ctx.timeZone).split(",")[0]}`,
        });
        continue;
      case "settled": {
        const gone = e.data.outcome.startsWith("let it go");
        rows.push({
          is: "mark",
          key,
          at,
          label: gone ? "let go of it" : "came back to it",
          text: gone ? e.data.outcome.replace(/^let it go:?\s*/, "") : e.data.outcome,
        });
        continue;
      }
      case "left":
        rows.push({ is: "will", key, at, verb: "left", text: e.data.why });
        continue;
      case "canon":
        rows.push({ is: "mark", key, at, label: "now true", text: e.data.fact });
        continue;
      default:
        break;
    }

    // Everything below happens to her alone and ends whatever episode was open.
    place = null;
    speaker = "";
    switch (e.kind) {
      case "born":
        rows.push({ is: "break", key, at, label: `born, and sealed ${e.data.seal.slice(0, 8)}` });
        break;
      case "woke":
        rows.push({ is: "break", key, at, label: "woke" });
        break;
      case "slept":
        rows.push({ is: "break", key, at, label: "went to sleep", night: true });
        break;
      case "dormant":
        rows.push({
          is: "break",
          key,
          at,
          label: `nothing from ${clock(e.data.since, ctx.timeZone)} to ${clock(at, ctx.timeZone)}: her body was not running`,
          night: true,
        });
        break;
      case "planned":
        plan = e.data.blocks.map((b) => b.what);
        rows.push({
          is: "mark",
          key,
          at,
          label: "planned her day",
          text: e.data.weather ? `under ${e.data.weather}` : `${e.data.blocks.length} things`,
        });
        break;
      case "lived":
        rows.push({ is: "lived", key, at, text: e.data.text, title: e.data.block === undefined ? undefined : plan[e.data.block] });
        break;
      case "reflected":
        rows.push({ is: "reflected", key, at, text: e.data.text });
        break;
      case "posted":
        rows.push({ is: "episode", key: `p${entry.seq}`, at, place: "", title: "to nobody in particular", meta: e.data.channel });
        rows.push({ is: "said", key, at, text: e.data.text });
        break;
      case "saw":
        rows.push({ is: "mark", key, at, label: `scrolled ${e.data.channel}`, text: e.data.text });
        break;
      default:
        break;
    }
  }
  return rows;
}
