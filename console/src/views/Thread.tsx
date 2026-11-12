// One place she is in, read as the trace of her mind: what people wrote on the
// right, what she said on the left, and between them what she made of it.
//
// Two things a chat window hides are shown here on purpose. Time: how long a
// message waited before she read it. And will: when she chose to say nothing,
// it is named as a choice, not left as an absence.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { deltaText, describe } from "../describe";
import { REACH_WORDS, STAGE_WORD, ago, clock, day, dayKey, signed, span, until } from "../format";
import * as Icon from "../icons";
import type { Entry, PersonRow, Status } from "../protocol/types";
import { say, talkKey, typingToHer, useStore } from "../store";

type Item =
  | { is: "day"; key: string; label: string }
  | { is: "them"; key: string; name: string | null; text: string; at: string; aside: boolean }
  | { is: "her"; key: string; text: string; at: string }
  | { is: "steps"; key: string; entries: Entry[] };

type Trace = {
  items: Item[];
  /** For each `read` entry, how long the oldest unread message had waited. */
  waited: Map<number, string>;
};

function build(entries: Entry[], timeZone: string | undefined, shared: boolean): Trace {
  const items: Item[] = [];
  const waited = new Map<number, string>();
  let today = "";
  let speaker = "";
  let unreadSince: string | null = null;
  for (const entry of entries) {
    const dk = dayKey(entry.at, timeZone);
    if (dk !== today) {
      today = dk;
      speaker = "";
      items.push({ is: "day", key: `d${dk}`, label: day(entry.at, timeZone) });
    }
    const e = entry.event;
    if (e.kind === "heard") {
      unreadSince ??= entry.at;
      // In a shared place, name whoever is speaking when the speaker changes.
      const name = shared && e.data.from.id !== speaker ? e.data.from.name : null;
      speaker = e.data.from.id;
      items.push({
        is: "them",
        key: `e${entry.seq}`,
        name,
        text: e.data.text,
        at: entry.at,
        aside: shared && !e.data.addressed,
      });
      continue;
    }
    speaker = "";
    if (e.kind === "said") {
      items.push({ is: "her", key: `e${entry.seq}`, text: e.data.text, at: entry.at });
      continue;
    }
    if (e.kind === "read" && unreadSince) {
      waited.set(entry.seq, span(unreadSince, entry.at));
      unreadSince = null;
    }
    const last = items[items.length - 1];
    if (last?.is === "steps") last.entries.push(entry);
    else items.push({ is: "steps", key: `s${entry.seq}`, entries: [entry] });
  }
  return { items, waited };
}

function Step({ entry, timeZone, waited }: { entry: Entry; timeZone: string | undefined; waited?: string }) {
  const [open, setOpen] = useState(false);
  const told = describe(entry, timeZone);
  const detail = entry.event.kind === "read" ? (waited ? `after ${waited}` : "") : told.detail;
  return (
    <button
      className={`step${open ? " is-open" : ""}`}
      onClick={() => setOpen(!open)}
      title={clock(entry.at, timeZone)}
    >
      <span className="step-verb">{told.verb}</span>
      {told.delta !== undefined ? <span className={`delta is-${told.tone}`}>{deltaText(told)}</span> : null}
      <span className={`step-detail${told.tone === "bad" && told.delta === undefined ? " is-bad" : ""}`}>
        {detail || clock(entry.at, timeZone)}
      </span>
      {open && told.more ? <span className="step-more">{told.more}</span> : null}
    </button>
  );
}

/** What she is doing about this place right now, from her agenda. */
function Presence({ placeKey }: { placeKey: string }) {
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
  if (agenda.thinking.includes(placeKey)) return <div className="presence shimmer">Reading</div>;
  const typingNow = typing !== undefined && Date.now() - typing < 6000;
  if (agenda.speaking.includes(placeKey) || typingNow) {
    return (
      <div className="presence">
        {name} is typing
        <span className="dots" aria-hidden="true">
          <i />
          <i />
          <i />
        </span>
      </div>
    );
  }
  const look = agenda.attend.find((a) => a.place === placeKey);
  if (look) {
    const asleep = status?.moment.phase === "asleep";
    return (
      <div className="presence">
        {asleep
          ? `${name} is asleep. She will see this after she wakes.`
          : `Not seen yet. ${name} will look ${until(look.due)}.`}
      </div>
    );
  }
  return null;
}

/** How reachable she is at this moment, in a line. */
function availability(status: Status | null): { asleep: boolean; line: string } {
  if (!status) return { asleep: false, line: "" };
  const { moment, timezone } = status;
  if (moment.phase === "asleep") {
    return { asleep: true, line: `Asleep until ${clock(moment.until, timezone)}. She will see this in the morning.` };
  }
  if (!moment.doing) return { asleep: false, line: "Nothing in particular going on. She may answer soon." };
  return { asleep: false, line: `${capital(moment.doing.what)}; ${REACH_WORDS[moment.reach]}.` };
}

function capital(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function Composer({ placeKey }: { placeKey: string }) {
  const settings = useStore((s) => s.settings);
  const talk = useStore((s) => s.talk);
  const status = useStore((s) => s.status);
  const ghost = useStore((s) => s.ghost);
  const places = useStore((s) => s.places);
  const [text, setText] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);
  const lastTyping = useRef(0);

  useLayoutEffect(() => {
    const area = box.current;
    if (!area) return;
    area.style.height = "auto";
    area.style.height = `${Math.min(area.scrollHeight, 200)}px`;
  }, [text]);

  if (placeKey !== talkKey(settings)) {
    const row = places.find((p) => p.place.key === placeKey);
    const who = row?.place.kind === "direct" ? (row.with ?? "they") : "the people here";
    return (
      <div className="composer-wrap">
        <div className="watching">
          <Icon.Eye />
          <span>
            You are watching. Only {who} and {ghost?.name ?? "she"} can speak here.
          </span>
        </div>
      </div>
    );
  }

  const ready = talk.is === "open";
  const send = () => {
    if (say(text)) setText("");
  };
  const { asleep, line } = availability(status);

  return (
    <div className="composer-wrap">
      <div className={`composer${ready ? "" : " is-off"}`}>
        <textarea
          ref={box}
          rows={1}
          value={text}
          disabled={!ready}
          placeholder={
            ready
              ? `Write to ${ghost?.name ?? "her"}`
              : talk.is === "refused"
                ? `You cannot talk from here: ${talk.why}`
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
        <div className="composer-row">
          {/* Not a model picker: when you write to a person, what matters is
              whether she is in a position to read it. */}
          <span className={`phase is-small ${asleep ? "is-night" : "is-day"}`}>
            {asleep ? <Icon.Moon /> : <Icon.Sun />}
          </span>
          <span className="composer-state">{line}</span>
          <button className="send" onClick={send} disabled={!ready || !text.trim()} aria-label="Send" title="Send">
            <Icon.ArrowUp />
          </button>
        </div>
      </div>
    </div>
  );
}

/** A line at the head of a conversation: who this is to her. */
function Context({ placeKey }: { placeKey: string }) {
  const places = useStore((s) => s.places);
  const people = useStore((s) => s.people);
  const settings = useStore((s) => s.settings);
  const name = useStore((s) => s.ghost?.name ?? "She");
  const row = places.find((p) => p.place.key === placeKey);
  const other = people.find((p) => p.person.direct === placeKey);
  const mine = placeKey === talkKey(settings);

  let line: string;
  if (mine && !other) line = `Your own conversation with ${name}. She will know you as ${settings.you.trim() || "you"}.`;
  else if (other) {
    const { standing, person } = other;
    line = `${mine ? "You are" : `${person.name} is`} ${article(STAGE_WORD[standing.stage])} to ${name}. She first heard from ${
      mine ? "you" : "them"
    } ${ago(person.first_at)} ago${mine ? "" : `, on ${person.channel}`}.`;
    if (standing.stage === "gone") line = `${name} has left ${mine ? "you" : person.name}. Nothing sent here reaches her.`;
  } else if (row?.place.kind === "group") line = `A group on ${row.place.channel}. Most of what is said here is not meant for ${name}.`;
  else if (row?.place.kind === "public") line = `A public thread on ${row.place.channel}. Anyone can read it.`;
  else return null;
  return <div className="context">{line.replace(" now ago", " just now")}</div>;
}

function article(word: string): string {
  if (word === "close") return "close";
  if (word === "left") return "someone she has left";
  if (word === "cooling") return "someone she is cooling toward";
  if (word === "estranged") return "estranged";
  return `${/^[aeiou]/.test(word) ? "an" : "a"} ${word}`;
}

export function Thread() {
  const thread = useStore((s) => s.thread);
  const status = useStore((s) => s.status);
  const ghost = useStore((s) => s.ghost);
  const places = useStore((s) => s.places);
  const settings = useStore((s) => s.settings);
  const timeZone = status?.timezone;
  const row = places.find((p) => p.place.key === thread.key);
  const shared = row !== undefined && row.place.kind !== "direct";
  const { items, waited } = useMemo(
    () => build(thread.entries, timeZone, shared),
    [thread.entries, timeZone, shared],
  );

  // Follow the conversation, unless the reader has scrolled up to look back.
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  });
  useEffect(() => {
    pinned.current = true;
  }, [thread.key]);

  const mine = thread.key === talkKey(settings);
  const empty = thread.loaded && items.length === 0;

  return (
    <section className="thread">
      <div
        className="thread-scroll"
        ref={scroller}
        onScroll={(event) => {
          const el = event.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        <div className="column">
          {empty ? (
            <div className="hello">
              <div className="hello-name">{ghost?.name ?? "Ghost"}</div>
              <div className="hello-line">
                {mine
                  ? "Nothing between you yet. Write, and she will answer when she gets to it."
                  : "Nothing has been said here yet."}
              </div>
            </div>
          ) : (
            <Context placeKey={thread.key} />
          )}
          {items.map((item) => {
            switch (item.is) {
              case "day":
                return (
                  <div key={item.key} className="day">
                    {item.label}
                  </div>
                );
              case "them":
                return (
                  <div key={item.key} className={`them${item.aside ? " is-aside" : ""}`}>
                    {item.name ? <div className="them-name">{item.name}</div> : null}
                    <div className="bubble" title={clock(item.at, timeZone)}>
                      {item.text}
                    </div>
                  </div>
                );
              case "her":
                return (
                  <div key={item.key} className="her" title={clock(item.at, timeZone)}>
                    {item.text}
                  </div>
                );
              case "steps":
                return (
                  <div key={item.key} className="steps">
                    {item.entries.map((entry) => (
                      <Step key={entry.seq} entry={entry} timeZone={timeZone} waited={waited.get(entry.seq)} />
                    ))}
                  </div>
                );
            }
          })}
          <Presence placeKey={thread.key} />
        </div>
      </div>
      <Composer placeKey={thread.key} />
    </section>
  );
}

/** Who is in this place, and what she holds about them. */
export function Inspector() {
  const thread = useStore((s) => s.thread);
  const people = useStore((s) => s.people);
  const places = useStore((s) => s.places);
  const status = useStore((s) => s.status);
  const name = status?.name ?? "She";
  const row = places.find((p) => p.place.key === thread.key);

  const here = new Set<string>();
  const open = new Map<number, Entry>();
  for (const entry of thread.entries) {
    const e = entry.event;
    if (e.kind === "heard") here.add(`${e.data.channel}:${e.data.from.id}`);
    else if (e.kind === "intended") open.set(entry.seq, entry);
    else if (e.kind === "settled") open.delete(e.data.intention);
  }
  const present = people.filter((p) => here.has(p.person.key));

  return (
    <aside className="inspector">
      {open.size > 0 ? (
        <>
          <div className="section is-first">{name} means to</div>
          {[...open.values()].map((entry) =>
            entry.event.kind === "intended" ? (
              <div key={entry.seq} className="intent">
                <div>{entry.event.data.what}</div>
                <div className="intent-when">{until(entry.event.data.due)}</div>
              </div>
            ) : null,
          )}
        </>
      ) : null}

      <div className={`section${open.size === 0 ? " is-first" : ""}`}>
        {present.length === 1 ? `Who this is to ${name}` : `Who is here, to ${name}`}
      </div>
      {present.length === 0 ? <div className="sidebar-empty">Nobody she has heard from yet.</div> : null}
      {present.map((person) => (
        <PersonCard key={person.person.key} row={person} timeZone={status?.timezone} />
      ))}

      {row && !row.online ? (
        <p className="inspector-note">
          {row.place.channel} is not connected right now, so nothing new can arrive here and she cannot answer.
        </p>
      ) : null}
    </aside>
  );
}

export function PersonCard({
  row,
  timeZone,
  onOpen,
}: {
  row: PersonRow;
  timeZone: string | undefined;
  onOpen?: () => void;
}) {
  const { person, standing, notes } = row;
  // Warmth runs from -20 to 20; the meter grows from the middle.
  const reach = Math.min(Math.abs(standing.warmth) / 20, 1) * 50;
  return (
    <div className="person">
      <div className="person-head">
        {onOpen ? (
          <button className="person-name is-link" onClick={onOpen}>
            {person.name}
          </button>
        ) : (
          <span className="person-name">{person.name}</span>
        )}
        {person.handle ? <span className="person-handle">@{person.handle}</span> : null}
        <span className={`stage is-${standing.stage}`}>{STAGE_WORD[standing.stage]}</span>
      </div>
      <div className="person-stats">
        {person.channel} · known {standing.known_days}d · talked on {standing.active_days}d
        {person.automated ? " · automated" : ""}
      </div>
      <div className="warmth" title={`warmth ${signed(standing.warmth)}`}>
        <div
          className={`warmth-fill ${standing.warmth < 0 ? "is-bad" : "is-good"}`}
          style={standing.warmth < 0 ? { right: "50%", width: `${reach}%` } : { left: "50%", width: `${reach}%` }}
        />
        <div className="warmth-mid" />
      </div>
      {notes.length > 0 ? (
        <ul className="notes">
          {notes.map((note) => (
            <li key={note.seq} title={clock(note.at, timeZone)}>
              {note.text}
            </li>
          ))}
        </ul>
      ) : (
        <div className="person-none">She has not written anything down about them.</div>
      )}
    </div>
  );
}
