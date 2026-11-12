// Her side of a conversation with you, as a list of things to draw.
//
// What you wrote goes in bubbles. What she said stands plain. Between them
// are the steps of her mind, kept small: how long your message waited before
// she read it, what she thought, when she chose to say nothing.

import { clock, day, dayKey, span } from "./format";
import type { Entry } from "./protocol/types";

export type Step = {
  key: string;
  at: string;
  verb: string;
  detail: string;
  /** Shown when the step is opened. */
  more?: string;
  delta?: number;
};

export type Item =
  | { is: "day"; key: string; label: string }
  | { is: "me"; key: string; at: string; text: string }
  | { is: "her"; key: string; at: string; text: string; last: boolean }
  | { is: "steps"; key: string; steps: Step[] };

function step(entry: Entry, waited: string | null, timeZone?: string): Step | null {
  const base = { key: `e${entry.seq}`, at: entry.at };
  const e = entry.event;
  switch (e.kind) {
    case "read":
      return waited ? { ...base, verb: "Read", detail: `after ${waited}` } : null;
    case "decided": {
      const { decision, reason } = e.data;
      if (decision.startsWith("stayed quiet")) {
        const why = decision.slice("stayed quiet".length).replace(/^[:;\s]+/, "");
        return { ...base, verb: "Stayed quiet", detail: why || reason, more: why && reason ? reason : undefined };
      }
      if (decision === "held back") return { ...base, verb: "Held back", detail: "you wrote again before she sent it" };
      if (decision === "could not send") return { ...base, verb: "Could not send", detail: reason };
      if (decision === "nothing") return reason ? { ...base, verb: "Let it pass", detail: reason } : null;
      return reason ? { ...base, verb: "Thought", detail: reason, more: reason } : null;
    }
    case "felt":
      return { ...base, verb: "Felt", detail: e.data.why, delta: e.data.delta };
    case "noted":
      return { ...base, verb: "Noted", detail: e.data.text };
    case "intended":
      return { ...base, verb: "Means to", detail: `${e.data.what}, by ${clock(e.data.due, timeZone)}` };
    case "canon":
      return { ...base, verb: "Now true", detail: e.data.fact };
    case "reacted":
      return { ...base, verb: "Reacted", detail: e.data.emoji };
    case "left":
      return { ...base, verb: "Left", detail: e.data.why };
    default:
      return null;
  }
}

export function build(entries: Entry[], timeZone?: string): Item[] {
  const items: Item[] = [];
  let today = "";
  let unreadSince: string | null = null;
  for (const entry of entries) {
    const dk = dayKey(entry.at, timeZone);
    if (dk !== today) {
      const first = today === "";
      today = dk;
      const isToday = dk === dayKey(new Date().toISOString(), timeZone);
      if (!first || !isToday) items.push({ is: "day", key: `d${dk}`, label: isToday ? "Today" : day(entry.at, timeZone) });
      else items.push({ is: "day", key: `d${dk}`, label: "Today" });
    }
    const e = entry.event;
    if (e.kind === "heard") {
      unreadSince ??= entry.at;
      items.push({ is: "me", key: `e${entry.seq}`, at: entry.at, text: e.data.text });
      continue;
    }
    if (e.kind === "said") {
      items.push({ is: "her", key: `e${entry.seq}`, at: entry.at, text: e.data.text, last: false });
      continue;
    }
    const waited = e.kind === "read" && unreadSince ? span(unreadSince, entry.at) : null;
    if (e.kind === "read") unreadSince = null;
    const made = step(entry, waited, timeZone);
    if (!made) continue;
    const last = items[items.length - 1];
    if (last?.is === "steps") last.steps.push(made);
    else items.push({ is: "steps", key: `s${entry.seq}`, steps: [made] });
  }
  // The last thing she said in a run carries the time and the small actions.
  for (let i = 0; i < items.length; i++) {
    const item = items[i]!;
    if (item.is === "her" && items[i + 1]?.is !== "her") item.last = true;
  }
  return items;
}
