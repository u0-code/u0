// A journal entry in a few words: a verb, and what it was about.

import { clock, signed } from "./format";
import type { Entry } from "./protocol/types";

export type Told = {
  verb: string;
  detail: string;
  /** A second line shown when the row is opened. */
  more?: string;
  /** A signed number to colour, as in a diff. */
  delta?: number;
  tone?: "good" | "bad";
};

export function describe(entry: Entry, timeZone?: string): Told {
  const e = entry.event;
  switch (e.kind) {
    case "born":
      return { verb: "Born", detail: `${e.data.name}, sealed ${e.data.seal.slice(0, 8)}` };
    case "dormant":
      return { verb: "Dormant", detail: `nothing since ${clock(e.data.since, timeZone)}` };
    case "woke":
      return { verb: "Woke", detail: "" };
    case "slept":
      return { verb: "Slept", detail: "" };
    case "planned":
      return {
        verb: "Planned",
        detail: e.data.blocks.map((b) => b.what).join(" · "),
        more: e.data.weather ?? undefined,
      };
    case "lived":
      return { verb: "Lived", detail: e.data.text };
    case "heard":
      return {
        verb: "Heard",
        detail: `${e.data.from.name}: ${e.data.text}`,
        more: `${e.data.channel} · ${e.data.place.name ?? e.data.place.id}`,
      };
    case "read":
      return { verb: "Read", detail: "" };
    case "said":
      return { verb: "Said", detail: e.data.text, more: `${e.data.channel} · ${e.data.place}` };
    case "reacted":
      return { verb: "Reacted", detail: e.data.emoji };
    case "posted":
      return { verb: "Posted", detail: e.data.text, more: e.data.channel };
    case "saw":
      return { verb: "Scrolled", detail: e.data.text, more: e.data.channel };
    case "decided":
      return decided(e.data.decision, e.data.reason);
    case "felt":
      return {
        verb: "Felt",
        detail: e.data.why,
        delta: e.data.delta,
        tone: e.data.delta > 0 ? "good" : "bad",
      };
    case "noted":
      return { verb: "Noted", detail: e.data.text };
    case "intended":
      return { verb: "Intends", detail: e.data.what, more: `by ${clock(e.data.due, timeZone)}` };
    case "settled":
      return { verb: "Settled", detail: e.data.outcome };
    case "left":
      return { verb: "Left", detail: e.data.why, tone: "bad" };
    case "reflected":
      return { verb: "Reflected", detail: e.data.text };
    case "canon":
      return { verb: "Canon", detail: e.data.fact, more: e.data.source };
  }
}

/** A decision is where her will shows. Not answering is named for what it is. */
function decided(decision: string, reason: string): Told {
  const rest = (prefix: string) => decision.slice(prefix.length).replace(/^[:;\s]+/, "");
  if (decision.startsWith("stayed quiet")) {
    return { verb: "Stayed quiet", detail: rest("stayed quiet") || reason, more: reason || undefined };
  }
  if (decision === "held back") return { verb: "Held back", detail: reason };
  if (decision === "stayed out of it") return { verb: "Stayed out", detail: reason };
  if (decision === "nothing") return { verb: "Let it pass", detail: reason };
  if (decision === "could not send") return { verb: "Could not send", detail: reason, tone: "bad" };
  return reason ? { verb: "Thought", detail: reason, more: decision } : { verb: "Decided", detail: decision };
}

export function deltaText(told: Told): string {
  return told.delta === undefined ? "" : signed(told.delta);
}
