// What the window says about a ghost, in words: who she is to grammar, how
// she is right now, and what is about to happen with what you wrote.

import { clock, dayKey, until } from "./format";
import type { Reach, Stage } from "./protocol/types";
import type { Session } from "./store";

/** Her pronouns, as the window needs them. */
export type Pronouns = { sub: string; obj: string; pos: string; is: string; has: string; s: string };

const KNOWN: Record<string, Pronouns> = {
  she: { sub: "she", obj: "her", pos: "her", is: "is", has: "has", s: "s" },
  he: { sub: "he", obj: "him", pos: "his", is: "is", has: "has", s: "s" },
  they: { sub: "they", obj: "them", pos: "their", is: "are", has: "have", s: "" },
  it: { sub: "it", obj: "it", pos: "its", is: "is", has: "has", s: "s" },
};

/** `she/her` → the forms of it. Anything unfamiliar is treated as `they`. */
export function pronouns(text: string | undefined): Pronouns {
  const first = (text ?? "").split("/")[0]?.trim().toLowerCase() ?? "";
  return KNOWN[first] ?? KNOWN.they!;
}

export function of(session: Session): Pronouns {
  return pronouns(session.status?.pronouns ?? session.local?.pronouns);
}

export const cap = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** How she is to be shown at a glance. */
export type Presence = "off" | "wait" | "ok" | "busy" | "asleep";

export function presence(session: Session): Presence {
  if (!session.url) return "off";
  if (session.watch?.is !== "open" || !session.status) return "wait";
  const { phase, reach } = session.status.moment;
  if (phase === "asleep") return "asleep";
  return reach === "free" || reach === "glance" ? "ok" : "busy";
}

/** Whether she is writing to you this moment. */
export function typing(session: Session, now: number): boolean {
  return now - session.typingAt < 9000;
}

/** The one line under her name in the list. */
export function line(session: Session, doing: string | undefined, now: number): string {
  if (doing) return `${doing}…`;
  if (!session.url) return session.local && !session.local.model ? "nothing to think with yet" : "not running";
  if (session.watch?.is === "refused") return session.watch.why;
  if (session.watch?.is !== "open" || !session.status) return "connecting…";
  if (typing(session, now)) return "typing…";
  const { moment, timezone } = session.status;
  if (moment.phase === "asleep") return `asleep until ${clock(moment.until, timezone)}`;
  return moment.doing?.what ?? "nothing planned";
}

/** Where and when she is, for the title bar: `Chengdu 17:02 · at her desk`. */
export function whereabouts(session: Session, now: number): string {
  const status = session.status;
  if (!status) return line(session, undefined, now);
  const here = `${status.city} ${clock(new Date(now).toISOString(), status.timezone)}`;
  const doing = status.moment.phase === "asleep" ? "asleep" : status.moment.doing?.what;
  return doing ? `${here} · ${doing}` : here;
}

/** What an activity leaves of her attention. */
function reach(r: Reach, p: Pronouns): string {
  switch (r) {
    case "free":
      return `${cap(p.pos)} phone is in ${p.pos} hand.`;
    case "glance":
      return `${cap(p.sub)} glance${p.s} at ${p.pos} phone now and then.`;
    case "busy":
      return `${cap(p.sub)} rarely look${p.s} at ${p.pos} phone.`;
    case "away":
      return `${cap(p.sub)} ${p.is} not looking at ${p.pos} phone.`;
  }
}

/** Her moment, as a sentence or two. */
export function moment(session: Session): string {
  const status = session.status;
  if (!status) return "";
  const p = of(session);
  const { moment: m, timezone } = status;
  if (m.phase === "asleep") return `Asleep until about ${clock(m.until, timezone)}.`;
  if (!m.doing) return `Nothing planned. ${reach(m.reach, p)}`;
  return `${cap(m.doing.what)} until ${clock(m.doing.to, timezone)}. ${reach(m.reach, p)}`;
}

/** What someone is to her, as the end of "She sees you as ___". */
export function standing(stage: Stage, p: Pronouns): string {
  switch (stage) {
    case "gone":
      return `someone ${p.sub} ${p.has} left`;
    case "estranged":
      return `someone ${p.sub} ${p.has} pulled away from`;
    case "cooling":
      return `someone ${p.sub} ${p.is} cooling toward`;
    case "stranger":
      return "a stranger";
    case "acquaintance":
      return "an acquaintance";
    case "friend":
      return "a friend";
    case "close":
      return "someone close";
  }
}

/** When something last passed between you, as the list shows it. */
export function stamp(at: string | undefined, timeZone: string | undefined, now: number): string {
  if (!at) return "";
  const then = new Date(at).getTime();
  if (dayKey(at, timeZone) === dayKey(new Date(now).toISOString(), timeZone)) return clock(at, timeZone);
  if (now - then < 6 * 86400000) {
    return new Intl.DateTimeFormat("en-GB", { weekday: "short", timeZone }).format(new Date(at));
  }
  return new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short", timeZone }).format(new Date(at));
}

/** What is happening with the last thing you wrote, if anything is. */
export type Live =
  | { is: "typing"; text: string }
  | { is: "wait"; text: string }
  | { is: "asleep"; text: string }
  | { is: "taken"; text: string };

export function live(session: Session, place: string, now: number): Live | null {
  const name = session.name;
  const p = of(session);
  if (session.talk?.is === "refused") return { is: "taken", text: "Another window is talking to " + p.obj + " now." };
  if (typing(session, now) || session.agenda?.speaking.includes(place)) return { is: "typing", text: `${name} is typing` };
  const last = session.entries[session.entries.length - 1];
  if (last?.event.kind !== "heard") return null;
  if (session.agenda?.thinking.includes(place)) return { is: "wait", text: `${name} is reading it now.` };
  const status = session.status;
  if (status?.moment.phase === "asleep") {
    return {
      is: "asleep",
      text: `${name} is asleep. ${cap(p.sub)} will see this after ${clock(status.moment.until, status.timezone)}.`,
    };
  }
  const due = session.agenda?.attend.find((a) => a.place === place)?.due;
  if (!due) return { is: "wait", text: "Not seen yet." };
  return { is: "wait", text: `Not seen yet. ${name} will look ${until(due, now)}.` };
}
