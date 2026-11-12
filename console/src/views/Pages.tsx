// The three views that are not a conversation: her journal as it is written,
// the people she knows, and her life.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { deltaText, describe } from "../describe";
import { REACH_WORDS, clock, day, stamp, until } from "../format";
import * as Icon from "../icons";
import type { Kind } from "../protocol/types";
import { show, useStore } from "../store";
import { PersonCard } from "./Thread";

const GROUPS: { label: string; kinds: Kind[] | null }[] = [
  { label: "Everything", kinds: null },
  { label: "Conversation", kinds: ["heard", "said", "read", "reacted"] },
  { label: "Mind", kinds: ["decided", "felt", "noted", "intended", "settled", "left"] },
  { label: "Life", kinds: ["born", "woke", "slept", "planned", "lived", "reflected", "canon", "dormant"] },
  { label: "Public", kinds: ["posted", "saw"] },
];

export function JournalPage() {
  const journal = useStore((s) => s.journal);
  const timeZone = useStore((s) => s.status?.timezone);
  const [group, setGroup] = useState(0);
  const kinds = GROUPS[group]?.kinds ?? null;
  const rows = useMemo(
    () => (kinds ? journal.filter((entry) => kinds.includes(entry.event.kind)) : journal),
    [journal, kinds],
  );

  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  });

  return (
    <section className="page">
      <div className="page-bar">
        {GROUPS.map((g, i) => (
          <button key={g.label} className={`tab${i === group ? " is-on" : ""}`} onClick={() => setGroup(i)}>
            {g.label}
          </button>
        ))}
        <span className="page-count">{rows.length} entries</span>
      </div>
      <div
        className="log"
        ref={scroller}
        onScroll={(event) => {
          const el = event.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        {rows.length === 0 ? <div className="page-empty">Nothing here yet.</div> : null}
        {rows.map((entry) => {
          const told = describe(entry, timeZone);
          return (
            <div key={entry.seq} className="log-row">
              <span className="log-seq">#{entry.seq}</span>
              <span className="log-time">{stamp(entry.at, timeZone)}</span>
              <span className="log-verb">{told.verb}</span>
              <span className="log-text">
                {told.delta !== undefined ? <span className={`delta is-${told.tone}`}>{deltaText(told)} </span> : null}
                {told.detail}
                {told.more ? <span className="log-more"> · {told.more}</span> : null}
              </span>
            </div>
          );
        })}
      </div>
    </section>
  );
}

export function PeoplePage() {
  const people = useStore((s) => s.people);
  const timeZone = useStore((s) => s.status?.timezone);
  const name = useStore((s) => s.ghost?.name ?? "She");
  return (
    <section className="page">
      <div className="page-scroll">
        <div className="column">
          <h1 className="page-title">People</h1>
          <p className="page-lede">
            Where {name} stands with everyone she has heard from. Nobody sets this, including her: it follows from how
            long she has known someone, how often they have actually talked, and what she has felt along the way.
          </p>
          {people.length === 0 ? <div className="page-empty">She does not know anyone yet.</div> : null}
          <div className="people">
            {people.map((row) => (
              <PersonCard
                key={row.person.key}
                row={row}
                timeZone={timeZone}
                onOpen={row.person.direct ? () => show({ is: "thread", key: row.person.direct! }) : undefined}
              />
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

export function LifePage() {
  const status = useStore((s) => s.status);
  const agenda = useStore((s) => s.agenda);
  const journal = useStore((s) => s.journal);
  const places = useStore((s) => s.places);
  const identity = useStore((s) => s.identity);
  const canon = useStore((s) => s.canon);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 5000);
    return () => clearInterval(timer);
  }, []);

  if (!status) return <section className="page" />;
  const timeZone = status.timezone;
  const { moment, today } = status;
  const asleep = moment.phase === "asleep";
  const lately = journal
    .filter((entry) => entry.event.kind === "lived" || entry.event.kind === "reflected")
    .slice(-6)
    .reverse();
  const title = (key: string) => {
    const row = places.find((p) => p.place.key === key);
    return row ? (row.place.kind === "direct" ? (row.with ?? row.place.id) : (row.place.name ?? key)) : key;
  };
  const seed = identity?.seed;

  return (
    <section className="page">
      <div className="page-scroll">
        <div className="column">
          <div className={`phase is-large ${asleep ? "is-night" : "is-day"}`}>
            {asleep ? <Icon.Moon /> : <Icon.Sun />}
          </div>
          <h1 className="page-title is-serif">{day(new Date(now).toISOString(), timeZone)}</h1>
          <p className="page-lede">
            It is {clock(new Date(now).toISOString(), timeZone)} in {status.city}
            {today?.weather ? `, ${today.weather}` : ""}.{" "}
            {asleep
              ? `${status.name} is asleep until about ${clock(moment.until, timeZone)}.`
              : moment.doing
                ? `${status.name} is ${moment.doing.what}; ${REACH_WORDS[moment.reach]}.`
                : `${status.name} is awake with nothing in particular going on.`}
          </p>

          <div className="section">Her day</div>
          {today && today.blocks.length > 0 ? (
            <div className="plan">
              {today.blocks.map((block) => {
                const from = new Date(block.from).getTime();
                const to = new Date(block.to).getTime();
                const when = to <= now ? "is-past" : from <= now ? "is-now" : "";
                return (
                  <div key={block.index} className={`plan-row ${when}`}>
                    <span className="plan-time">
                      {clock(block.from, timeZone)}–{clock(block.to, timeZone)}
                    </span>
                    <span className="plan-what">{block.what}</span>
                    <span className={`reach is-${block.reach}`} title={REACH_WORDS[block.reach]}>
                      {block.reach}
                    </span>
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="page-empty">She has not planned today.</div>
          )}

          <div className="section">What she will do next, unasked</div>
          <div className="upcoming">
            {agenda?.attend.map((look) => (
              <div key={look.place} className="upcoming-row">
                <span>Look at {title(look.place)}</span>
                <span className="upcoming-when">{asleep ? "after she wakes" : until(look.due, now)}</span>
              </div>
            ))}
            {agenda?.intention ? (
              <div className="upcoming-row">
                <span>Act on something she means to do</span>
                <span className="upcoming-when">{until(agenda.intention, now)}</span>
              </div>
            ) : null}
            {agenda?.phone ? (
              <div className="upcoming-row">
                <span>Pick up her phone to scroll</span>
                <span className="upcoming-when">{until(agenda.phone, now)}</span>
              </div>
            ) : null}
            {agenda && agenda.attend.length === 0 && !agenda.intention && !agenda.phone ? (
              <div className="page-empty">Nothing is waiting on her.</div>
            ) : null}
          </div>

          {lately.length > 0 ? (
            <>
              <div className="section">In her own words</div>
              <div className="lately">
                {lately.map((entry) => (
                  <div key={entry.seq} className="lately-row">
                    <span className="lately-time">{stamp(entry.at, timeZone).slice(0, 11)}</span>
                    <span className="is-serif">{describe(entry, timeZone).detail}</span>
                  </div>
                ))}
              </div>
            </>
          ) : null}

          <div className="section">What is true about her life</div>
          {canon.length === 0 ? (
            <div className="page-empty">
              Nothing yet beyond who she is. Facts gather here as she lives and as she tells people things.
            </div>
          ) : (
            <ul className="canon">
              {canon.map((fact) => (
                <li key={fact.seq} title={`${stamp(fact.at, timeZone)} · ${fact.source}`}>
                  {fact.fact}
                </li>
              ))}
            </ul>
          )}

          {seed ? (
            <>
              <div className="section">Who she is</div>
              <article className="certificate">
                <header className="certificate-head">
                  <span className="certificate-name is-serif">{seed.name}</span>
                  <span className="faint">
                    {seed.pronouns} · born {seed.born} · {seed.home.city}, {seed.home.country} · wakes {seed.rhythm.wake},
                    sleeps {seed.rhythm.sleep}
                  </span>
                </header>
                <p className="is-serif">{prose(seed.temperament)}</p>
                <h3>How she talks</h3>
                <p className="is-serif">{prose(seed.voice)}</p>
                <h3>Where she came from</h3>
                <p className="is-serif">{prose(seed.origin)}</p>
                {(
                  [
                    ["Values", seed.values],
                    ["Cares about", seed.cares_about],
                    ["Cannot stand", seed.cannot_stand],
                  ] as const
                ).map(([label, items]) =>
                  items.length > 0 ? (
                    <div key={label} className="certificate-list">
                      <h3>{label}</h3>
                      <div className="chips">
                        {items.map((item) => (
                          <span key={item} className="chip">
                            {item}
                          </span>
                        ))}
                      </div>
                    </div>
                  ) : null,
                )}
                <footer className="certificate-seal">
                  <Icon.Seal />
                  <span>
                    Sealed at birth. Nobody can edit this, including whoever wrote it.
                    <span className="mono"> {identity.seal.slice(0, 16)}</span>
                  </span>
                </footer>
              </article>
            </>
          ) : null}
        </div>
      </div>
    </section>
  );
}

/** A certificate is written with hard line breaks; read it as paragraphs. */
function prose(text: string): string {
  return text
    .trim()
    .split(/\n\s*\n/)
    .map((paragraph) => paragraph.replace(/\s*\n\s*/g, " "))
    .join("\n\n");
}
