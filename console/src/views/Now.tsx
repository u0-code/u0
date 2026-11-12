// The left of the window: her, at this moment.
//
// Who she is, what time it is where she lives, what she is in the middle of,
// and her day laid out down the page with a mark where she is in it. Time is
// the spine of this column and the way around the record beside it.

import { useEffect, useState } from "react";
import { Sigil } from "../Sigil";
import { REACH, STANDING, ago, clock, day, dayKey, lastActive, noonOf, until } from "../format";
import type { Entry } from "../protocol/types";
import { dayOf, look, open, talkKey, useStore } from "../store";

type Stretch = { from: string; to: string; what: string; at?: string; when: "past" | "now" | "ahead" };

/** The stretches of the day being shown: resolved for today, as written for other days. */
function stretches(
  shown: string,
  today: string,
  journal: Entry[],
  timeZone: string | undefined,
  now: number,
  live: { from: string; to: string; what: string }[] | undefined,
): Stretch[] {
  if (shown === today && live) {
    return live.map((b) => {
      const [from, to] = [new Date(b.from).getTime(), new Date(b.to).getTime()];
      return {
        from: clock(b.from, timeZone),
        to: clock(b.to, timeZone),
        what: b.what,
        at: b.from,
        when: to <= now ? "past" : from <= now ? "now" : "ahead",
      };
    });
  }
  for (let i = journal.length - 1; i >= 0; i--) {
    const e = journal[i]!.event;
    if (e.kind === "planned" && e.data.day === shown) {
      return e.data.blocks.map((b) => ({ from: b.from, to: b.to, what: b.what, when: "past" as const }));
    }
  }
  return [];
}

/** Bring the record to a moment of the day. */
function scrollTo(at: string | undefined) {
  if (!at) return;
  const rows = document.querySelectorAll<HTMLElement>(".record [data-at]");
  for (const row of rows) {
    if ((row.dataset.at ?? "") >= at) {
      row.scrollIntoView({ behavior: "smooth", block: "start" });
      return;
    }
  }
  rows[rows.length - 1]?.scrollIntoView({ behavior: "smooth", block: "end" });
}

export function Now() {
  const ghost = useStore((s) => s.ghost);
  const status = useStore((s) => s.status);
  const identity = useStore((s) => s.identity);
  const agenda = useStore((s) => s.agenda);
  const places = useStore((s) => s.places);
  const people = useStore((s) => s.people);
  const journal = useStore((s) => s.journal);
  const scope = useStore((s) => s.scope);
  const settings = useStore((s) => s.settings);
  const watch = useStore((s) => s.watch);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 5000);
    return () => clearInterval(timer);
  }, []);

  if (!ghost || !status) return <aside className="now" />;
  const timeZone = status.timezone;
  const nowIso = new Date(now).toISOString();
  const today = dayKey(nowIso, timeZone);
  const shown = dayOf(scope, timeZone);
  const asleep = status.moment.phase === "asleep";
  const mine = talkKey(settings);

  // The days her loaded journal covers, for walking back and forth.
  const days = [...new Set(journal.map((entry) => dayKey(entry.at, timeZone)))];
  if (!days.includes(today)) days.push(today);
  const at = days.indexOf(shown);
  const [earlier, later] = [days[at - 1], days[at + 1]];

  const blocks = stretches(shown, today, journal, timeZone, now, status.today?.blocks);
  const born = identity?.seed.born;
  const age = born ? Math.max(1, Math.round((Date.parse(noonOf(today)) - Date.parse(noonOf(born))) / 86400000) + 1) : null;

  /** What she is about to do about a place, in a few words. */
  const looks = (key: string): string | null => {
    if (agenda?.thinking.includes(key)) return "she is reading it now";
    if (agenda?.speaking.includes(key)) return "she is writing back";
    const due = agenda?.attend.find((a) => a.place === key)?.due;
    if (!due) return null;
    return asleep ? "she will see it when she wakes" : `she will look ${until(due, now)}`;
  };

  const others = places.filter((row) => row.place.key !== mine);
  const you = places.find((row) => row.place.key === mine);

  return (
    <aside className="now">
      <button className="who" onClick={() => open("certificate")} title="Her birth certificate">
        <Sigil seal={ghost.seal} size={46} />
        <span className="who-text">
          <span className="who-name">{ghost.name}</span>
          <span className="label">
            {ghost.pronouns} · {ghost.city}, {ghost.country}
          </span>
        </span>
      </button>

      <div className="moment">
        <div className="moment-time">{clock(nowIso, timeZone)}</div>
        <div className="label">{day(nowIso, timeZone)}</div>
        {status.today?.weather ? <div className="label">{status.today.weather}</div> : null}
        <p className="moment-doing">
          {asleep
            ? `Asleep, until about ${clock(status.moment.until, timeZone)}.`
            : status.moment.doing
              ? `${status.moment.doing.what.replace(/^./, (c) => c.toUpperCase())}.`
              : "Awake, with nothing in particular going on."}
        </p>
        {!asleep ? <div className="moment-reach">{REACH[status.moment.reach]}</div> : null}
      </div>

      {age ? (
        <div className="label tally">
          day {age} · {status.journal} entries · {people.length} {people.length === 1 ? "person" : "people"}
        </div>
      ) : null}

      <section className="daybook">
        <header className="daybook-head">
          <button
            className={`label link${scope.is === "day" ? " is-on" : ""}`}
            onClick={() => look({ is: "day", day: shown === today ? null : shown })}
          >
            {shown === today ? "today" : day(noonOf(shown), "UTC")}
          </button>
          <span className="daybook-walk">
            <button
              className="step-button"
              disabled={!earlier}
              onClick={() => earlier && look({ is: "day", day: earlier })}
              aria-label="The day before"
            >
              ‹
            </button>
            <button
              className="step-button"
              disabled={!later}
              onClick={() => later && look({ is: "day", day: later === today ? null : later })}
              aria-label="The day after"
            >
              ›
            </button>
          </span>
        </header>
        {blocks.length === 0 ? (
          <p className="quiet">{shown === today ? "She has not planned today." : "Nothing was planned that day."}</p>
        ) : (
          <ol className="dayline">
            {blocks.map((block, i) => (
              <li key={i} className={`dayline-row is-${block.when}`}>
                <button onClick={() => scrollTo(block.at)} disabled={!block.at || scope.is !== "day"}>
                  <span className="dayline-time">{block.from}</span>
                  <span className="dayline-what">{block.what}</span>
                </button>
              </li>
            ))}
            {identity ? (
              <li className={`dayline-row is-sleep${asleep && shown === today ? " is-now" : ""}`}>
                <div className="dayline-rest">
                  <span className="dayline-time">{identity.seed.rhythm.sleep}</span>
                  <span className="dayline-what">sleep</span>
                </div>
              </li>
            ) : null}
          </ol>
        )}
      </section>

      <section className="with">
        <header className="label">with</header>
        <button
          className={`with-row${scope.is === "place" && scope.key === mine ? " is-on" : ""}`}
          onClick={() => look({ is: "place", key: mine })}
        >
          <span className="with-name">You</span>
          <span className="with-note">
            {(you && you.place.unread > 0 && looks(mine)) || (you ? ago(lastActive(you), now) : "write to her")}
          </span>
        </button>
        {others.map((row) => {
          const person = people.find((p) => p.person.direct === row.place.key);
          const waiting = row.place.unread > 0 ? (looks(row.place.key) ?? "not seen yet") : null;
          const what =
            row.place.kind === "direct"
              ? person
                ? STANDING[person.standing.stage]
                : "someone"
              : row.place.kind === "group"
                ? "a group"
                : "in public";
          return (
            <button
              key={row.place.key}
              className={`with-row${scope.is === "place" && scope.key === row.place.key ? " is-on" : ""}`}
              onClick={() => look({ is: "place", key: row.place.key })}
            >
              <span className="with-name">
                {row.place.kind === "direct" ? (row.with ?? row.place.id) : (row.place.name ?? "a thread")}
              </span>
              <span className="with-note">
                {waiting ?? `${what} · ${row.place.channel}`}
              </span>
            </button>
          );
        })}
      </section>

      <button className="bodyline" onClick={() => open("body")} title="Her body">
        <span className={`pulse${watch.is === "open" ? " is-live" : ""}`} />
        <span className="label">
          body · {status.brains.think.split("/").pop()} ·{" "}
          {status.channels.filter((c) => c.channel !== "console").map((c) => c.channel).join(" ") || "this window only"}
        </span>
      </button>
    </aside>
  );
}
