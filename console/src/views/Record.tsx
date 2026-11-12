// The right of the window: her record.
//
// Three columns, like a ledger or a script: when, what kind of thing, and the
// thing itself. Other people's words carry their name. The steps of her mind
// carry a small label. Her own words carry nothing and are set largest.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { REACH, STANDING, ago, capital, clock, day, dayKey, noonOf, until } from "../format";
import type { Status } from "../protocol/types";
import { compose, placeTitle, type Row } from "../record";
import { dayOf, look, say, talkFromHere, talkKey, typingToHer, useStore } from "../store";

function Line({ row, time }: { row: Row; time: string }) {
  switch (row.is) {
    case "break":
      return (
        <div className={`line is-break${row.night ? " is-night" : ""}`} data-at={row.at}>
          <span className="line-time">{time}</span>
          <span className="break">
            <span className="label">{row.label}</span>
          </span>
        </div>
      );
    case "episode":
      return (
        <div className="line is-episode" data-at={row.at}>
          <span className="line-time">{time}</span>
          <span className="line-label" />
          <span className="episode">
            {row.place ? (
              <button className="episode-title" onClick={() => look({ is: "place", key: row.place })}>
                {row.title}
              </button>
            ) : (
              <span className="episode-title">{row.title}</span>
            )}
            <span className="label">{row.meta}</span>
          </span>
        </div>
      );
    case "heard":
      return (
        <div className={`line is-heard${row.aside ? " is-aside" : ""}`} data-at={row.at}>
          <span className="line-time">{time}</span>
          <span className="line-label is-name">{row.who}</span>
          <span className="line-text">{row.text}</span>
        </div>
      );
    case "said":
      return (
        <div className="line is-said" data-at={row.at}>
          <span className="line-time">{time}</span>
          <span className="line-label" />
          <span className="line-text">{row.text}</span>
        </div>
      );
    case "thought":
      return (
        <div className="line is-thought" data-at={row.at} title={row.did}>
          <span className="line-time">{time}</span>
          <span className="line-label">thought</span>
          <span className="line-text">{row.text}</span>
        </div>
      );
    case "will":
      return (
        <div className="line is-will" data-at={row.at}>
          <span className="line-time">{time}</span>
          <span className="line-label">{row.verb}</span>
          <span className="line-text">{row.text}</span>
        </div>
      );
    case "mark":
      return (
        <div className="line is-mark" data-at={row.at}>
          <span className="line-time">{time}</span>
          <span className="line-label">{row.label}</span>
          <span className="line-text">
            {row.delta !== undefined ? <span className="delta">{row.delta > 0 ? `+${row.delta}` : row.delta}</span> : null}
            {row.text}
          </span>
        </div>
      );
    case "lived":
      return (
        <div className="line is-lived" data-at={row.at}>
          <span className="line-time">{time}</span>
          <span className="line-label">lived</span>
          <span className="line-text">
            {row.title ? <span className="lived-title">{row.title}</span> : null}
            {row.text}
          </span>
        </div>
      );
    case "reflected":
      return (
        <div className="line is-reflected" data-at={row.at}>
          <span className="line-time">{time}</span>
          <span className="line-label">before sleep</span>
          <span className="line-text">{row.text}</span>
        </div>
      );
  }
}

/** What she is about to do about a place, from her agenda. */
function Pending({ placeKey, named }: { placeKey: string; named?: string }) {
  const agenda = useStore((s) => s.agenda);
  const status = useStore((s) => s.status);
  const typing = useStore((s) => s.typing[placeKey]);
  const name = status?.name ?? "She";
  const [, tick] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(timer);
  }, []);
  if (!agenda) return null;

  const about = named ? `${capital(named)}: ` : "";
  let text: string | null = null;
  let alive = false;
  if (agenda.thinking.includes(placeKey)) {
    text = `${about}${name} is reading.`;
    alive = true;
  } else if (agenda.speaking.includes(placeKey) || (typing !== undefined && Date.now() - typing < 6000)) {
    text = `${about}${name} is writing.`;
    alive = true;
  } else {
    const due = agenda.attend.find((a) => a.place === placeKey)?.due;
    if (due) {
      text =
        status?.moment.phase === "asleep"
          ? `${about}not seen yet. ${name} is asleep and will see it after she wakes.`
          : `${about}not seen yet. ${name} will look ${until(due)}.`;
    }
  }
  if (!text) return null;
  return (
    <div className="line is-pending">
      <span className="line-time" />
      <span className="line-label">{alive ? <span className="pulse is-live" /> : null}</span>
      <span className="line-text">{text}</span>
    </div>
  );
}

function availability(status: Status | null): string {
  if (!status) return "";
  const { moment, timezone } = status;
  if (moment.phase === "asleep") return `She is asleep until about ${clock(moment.until, timezone)} and will see this then.`;
  if (!moment.doing) return "She is awake with nothing in particular going on.";
  return `She is ${moment.doing.what}; ${REACH[moment.reach]}.`;
}

function Composer() {
  const talk = useStore((s) => s.talk);
  const status = useStore((s) => s.status);
  const ghost = useStore((s) => s.ghost);
  const [text, setText] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);
  const lastTyping = useRef(0);

  useLayoutEffect(() => {
    const area = box.current;
    if (!area) return;
    area.style.height = "auto";
    area.style.height = `${Math.min(area.scrollHeight, 180)}px`;
  }, [text]);

  const ready = talk.is === "open";
  const send = () => {
    if (say(text)) setText("");
  };

  return (
    <footer className="write">
      <div className="write-row">
        <textarea
          ref={box}
          rows={1}
          value={text}
          disabled={!ready}
          placeholder={
            ready
              ? `Write to ${ghost?.name ?? "her"}`
              : talk.is === "refused"
                ? "Another window is talking to her now. This one can only read."
                : "Connecting"
          }
          onChange={(event) => {
            setText(event.target.value);
            if (Date.now() - lastTyping.current > 3000) {
              lastTyping.current = Date.now();
              typingToHer();
            }
          }}
          onKeyDown={(event) => {
            // Enter sends; Shift+Enter breaks the line. Never while an input
            // method is still composing a character.
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              send();
            }
          }}
        />
        {talk.is === "refused" ? (
          <button className="write-send label" onClick={talkFromHere}>
            talk from here
          </button>
        ) : (
          <button className="write-send label" onClick={send} disabled={!ready || !text.trim()}>
            send ↵
          </button>
        )}
      </div>
      <div className="write-note">{availability(status)}</div>
    </footer>
  );
}

/** The head of a conversation: who this is, to her. */
function PlaceHead({ placeKey }: { placeKey: string }) {
  const places = useStore((s) => s.places);
  const people = useStore((s) => s.people);
  const settings = useStore((s) => s.settings);
  const name = useStore((s) => s.ghost?.name ?? "She");
  const mine = talkKey(settings);
  const { title, meta } = placeTitle(placeKey, { places, mine });
  const other = people.find((p) => p.person.direct === placeKey);
  const row = places.find((p) => p.place.key === placeKey);

  let line = "";
  if (other) {
    const { standing, person } = other;
    const who = placeKey === mine ? "You are" : `${person.name} is`;
    line =
      standing.stage === "gone"
        ? `${name} has left ${placeKey === mine ? "you" : person.name}. Nothing written here reaches her any more.`
        : `${who} ${STANDING[standing.stage]} to ${name}. She first heard from ${
            placeKey === mine ? "you" : "them"
          } ${ago(person.first_at)}.`;
    if (standing.warmth >= 3) line += ` She has warmed to ${placeKey === mine ? "you" : "them"}.`;
    else if (standing.warmth <= -3) line += ` ${placeKey === mine ? "You have" : "They have"} been wearing on her.`;
  } else if (placeKey === mine) {
    line = `Your own conversation with ${name}. She will know you as ${settings.you.trim() || "you"}.`;
  } else if (row?.place.kind === "group") {
    line = `A group. Most of what is said here is not meant for ${name}, and she knows it.`;
  } else if (row?.place.kind === "public") {
    line = "A public thread. Anyone can read what is written here.";
  }

  return (
    <header className="record-head">
      <button className="label link" onClick={() => look({ is: "day", day: null })}>
        ‹ her day
      </button>
      <h1>{capital(title)}</h1>
      <p className="record-lede">
        {line} <span className="label">{meta}</span>
      </p>
    </header>
  );
}

export function Record() {
  const scope = useStore((s) => s.scope);
  const status = useStore((s) => s.status);
  const journal = useStore((s) => s.journal);
  const thread = useStore((s) => s.thread);
  const places = useStore((s) => s.places);
  const people = useStore((s) => s.people);
  const agenda = useStore((s) => s.agenda);
  const settings = useStore((s) => s.settings);
  const identity = useStore((s) => s.identity);
  const timeZone = status?.timezone;
  const mine = talkKey(settings);
  const today = dayKey(new Date().toISOString(), timeZone);
  const shown = dayOf(scope, timeZone);

  const entries = useMemo(
    () =>
      scope.is === "place"
        ? thread.key === scope.key
          ? thread.entries
          : []
        : journal.filter((entry) => dayKey(entry.at, timeZone) === shown),
    [scope, thread, journal, timeZone, shown],
  );
  const rows = useMemo(
    () => compose(entries, { timeZone, places, people, mine, single: scope.is === "place" }),
    [entries, timeZone, places, people, mine, scope.is],
  );

  // Stay with the newest line, unless the reader has scrolled up to look back.
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const scopeKey = scope.is === "place" ? scope.key : `day:${shown}`;
  useEffect(() => {
    pinned.current = true;
  }, [scopeKey]);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  });

  const born = identity?.seed.born;
  const age = born ? Math.round((Date.parse(noonOf(shown)) - Date.parse(noonOf(born))) / 86400000) + 1 : null;
  const loading = scope.is === "place" && !(thread.loaded && thread.key === scope.key);
  let last = "";

  return (
    <section className="record">
      <div
        className="record-scroll"
        ref={scroller}
        onScroll={(event) => {
          const el = event.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        <div className="record-page">
          {scope.is === "place" ? (
            <PlaceHead placeKey={scope.key} />
          ) : (
            <header className="record-head">
              <span className="label">{age && age > 0 ? `day ${age} of her life` : "her record"}</span>
              <h1>{day(noonOf(shown), "UTC")}</h1>
            </header>
          )}

          {rows.length === 0 && !loading ? (
            <p className="quiet record-empty">
              {scope.is === "place"
                ? scope.key === mine
                  ? "Nothing between you yet. Write, and she will answer when she gets to it."
                  : "Nothing has been said here yet."
                : shown === today
                  ? "Nothing has happened yet today."
                  : "Nothing is recorded for this day."}
            </p>
          ) : null}

          {rows.map((row) => {
            // The time is printed once per minute, the way a log keeps its margin clean.
            const at = clock(row.at, timeZone);
            const time = at === last ? "" : at;
            last = at;
            return <Line key={row.key} row={row} time={time} />;
          })}

          {scope.is === "place" ? (
            <Pending placeKey={scope.key} />
          ) : shown === today ? (
            agenda?.attend
              .concat(agenda.thinking.map((place) => ({ place, due: "", addressed: true })))
              .filter((look, i, all) => all.findIndex((other) => other.place === look.place) === i)
              .map((look) => (
                <Pending key={look.place} placeKey={look.place} named={placeTitle(look.place, { places, mine }).title} />
              ))
          ) : null}
        </div>
      </div>
      {scope.is === "place" && scope.key === mine ? (
        <Composer />
      ) : scope.is === "place" ? (
        <footer className="write is-reading">
          <div className="write-note">You are reading. Only the people in this conversation and she can write here.</div>
        </footer>
      ) : null}
    </section>
  );
}
