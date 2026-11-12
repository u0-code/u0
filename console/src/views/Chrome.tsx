// The frame around everything.
//
// The sidebar reads from top to bottom as a sentence about her: who she is
// and what she is in the middle of; two ways of looking at her life; your own
// conversation with her; the conversations waiting on her; the rest; and, set
// apart at the bottom, the body she runs on. The body is the only part of
// this window that belongs to whoever is sitting at it.

import { useEffect, useRef, useState } from "react";
import { ago, clock, lastActive, placeTitle, shortly } from "../format";
import * as Icon from "../icons";
import type { PlaceKind, PlaceRow } from "../protocol/types";
import {
  CHANNEL,
  saveSettings,
  show,
  talkKey,
  toggleInspector,
  toggleSidebar,
  useStore,
  type View,
} from "../store";

export function KindIcon({ kind }: { kind: PlaceKind }) {
  return kind === "direct" ? <Icon.Person /> : kind === "group" ? <Icon.People /> : <Icon.Public />;
}

export function TitleBar() {
  const view = useStore((s) => s.view);
  const places = useStore((s) => s.places);
  const ghost = useStore((s) => s.ghost);
  const settings = useStore((s) => s.settings);
  const inspector = useStore((s) => s.inspector);
  const name = ghost?.name ?? "Ghost";

  let title = name;
  let icon: React.ReactNode = null;
  if (view.is === "journal") title = `${name}'s journal`;
  else if (view.is === "people") title = `People ${name} knows`;
  else if (view.is === "life") title = `${name}'s life`;
  else if (view.key === talkKey(settings)) {
    title = `You and ${name}`;
    icon = <Icon.Talk />;
  } else {
    const row = places.find((p) => p.place.key === view.key);
    title = row ? placeTitle(row) : view.key;
    icon = row ? <KindIcon kind={row.place.kind} /> : null;
  }

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="titlebar-left" data-tauri-drag-region>
        <button className="icon-button" onClick={toggleSidebar} title="Toggle sidebar" aria-label="Toggle sidebar">
          <Icon.Sidebar />
        </button>
      </div>
      <div className="titlebar-title" data-tauri-drag-region>
        {icon}
        <span>{title}</span>
      </div>
      <div className="titlebar-right" data-tauri-drag-region>
        {view.is === "thread" ? (
          <button
            className={`icon-button${inspector ? " is-on" : ""}`}
            onClick={toggleInspector}
            title="Who is here"
            aria-label="Who is here"
          >
            <Icon.Panel />
          </button>
        ) : null}
      </div>
    </header>
  );
}

/** Her, right now. The top of the sidebar, and the way into her life. */
function Her() {
  const ghost = useStore((s) => s.ghost);
  const status = useStore((s) => s.status);
  const on = useStore((s) => s.view.is === "life");
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 15000);
    return () => clearInterval(timer);
  }, []);

  const asleep = status?.moment.phase === "asleep";
  const where = status
    ? `${clock(new Date(now).toISOString(), status.timezone)} in ${status.city}`
    : (ghost?.city ?? "");
  const doing = !status
    ? ""
    : asleep
      ? `asleep until ${clock(status.moment.until, status.timezone)}`
      : (status.moment.doing?.what ?? "nothing in particular");

  return (
    <button className={`her-card${on ? " is-on" : ""}`} onClick={() => show({ is: "life" })} title="Her life">
      <span className={`phase ${asleep ? "is-night" : "is-day"}`}>{asleep ? <Icon.Moon /> : <Icon.Sun />}</span>
      <span className="her-text">
        <span className="her-name">{ghost?.name ?? "Ghost"}</span>
        <span className="her-where">{where}</span>
        <span className="her-doing">{doing}</span>
      </span>
    </button>
  );
}

function PlaceRowButton({
  row,
  active,
  aside,
}: {
  row: PlaceRow;
  active: boolean;
  /** What goes on the right: when she will look, or how long ago. */
  aside: React.ReactNode;
}) {
  return (
    <button className={`row${active ? " is-on" : ""}`} onClick={() => show({ is: "thread", key: row.place.key })}>
      <KindIcon kind={row.place.kind} />
      <span className="row-label">
        {placeTitle(row)}
        <span className="row-channel">{row.place.channel}</span>
      </span>
      {aside}
    </button>
  );
}

export function SideBar() {
  const ghost = useStore((s) => s.ghost);
  const status = useStore((s) => s.status);
  const agenda = useStore((s) => s.agenda);
  const places = useStore((s) => s.places);
  const view = useStore((s) => s.view);
  const settings = useStore((s) => s.settings);
  const [query, setQuery] = useState("");
  const [searching, setSearching] = useState(false);
  const mine = talkKey(settings);
  const now = Date.now();
  const active = view.is === "thread" ? view.key : null;
  const name = ghost?.name ?? "her";

  /** When she will get to a place, in a word. */
  const looks = (key: string): string => {
    if (agenda?.thinking.includes(key)) return "reading";
    if (agenda?.speaking.includes(key)) return "typing";
    const look = agenda?.attend.find((a) => a.place === key);
    if (!look) return "";
    return status?.moment.phase === "asleep" ? "morning" : shortly(look.due, now);
  };

  const others = places.filter(
    (row) => row.place.key !== mine && (!query || placeTitle(row).toLowerCase().includes(query.toLowerCase())),
  );
  const waiting = others.filter((row) => row.place.unread > 0);
  const rest = others.filter((row) => row.place.unread === 0);
  const own = places.find((row) => row.place.key === mine);
  const lenses: { view: View; label: string; icon: React.ReactNode }[] = [
    { view: { is: "journal" }, label: "Journal", icon: <Icon.Notebook /> },
    { view: { is: "people" }, label: "People", icon: <Icon.People /> },
  ];

  return (
    <aside className="sidebar">
      <Her />

      <div className="lenses">
        {lenses.map((lens) => (
          <button
            key={lens.label}
            className={`row${view.is === lens.view.is ? " is-on" : ""}`}
            onClick={() => show(lens.view)}
          >
            {lens.icon}
            <span className="row-label">{lens.label}</span>
          </button>
        ))}
      </div>

      <div className="section">You</div>
      <button className={`row${active === mine ? " is-on" : ""}`} onClick={() => show({ is: "thread", key: mine })}>
        <Icon.Talk />
        <span className="row-label">You and {name}</span>
        {own && own.place.unread > 0 ? <span className="row-soon">{looks(mine) || "unread"}</span> : null}
      </button>

      <div className="sidebar-scroll">
        {waiting.length > 0 ? (
          <>
            <div className="section" title="She has not looked at these yet">
              Waiting on {name}
            </div>
            {waiting.map((row) => (
              <PlaceRowButton
                key={row.place.key}
                row={row}
                active={active === row.place.key}
                aside={<span className="row-soon">{looks(row.place.key) || `${row.place.unread} new`}</span>}
              />
            ))}
          </>
        ) : null}

        <div className="section section-with-action">
          <span>Conversations</span>
          <button
            className={`icon-button is-small${searching ? " is-on" : ""}`}
            onClick={() => {
              setSearching(!searching);
              setQuery("");
            }}
            title="Find a conversation"
            aria-label="Find a conversation"
          >
            <Icon.Search />
          </button>
        </div>
        {searching ? (
          <input
            className="sidebar-search"
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Find a conversation"
          />
        ) : null}
        {rest.length === 0 && waiting.length === 0 ? (
          <div className="sidebar-empty">{query ? "Nothing matches." : "Nobody else has written to her yet."}</div>
        ) : null}
        {rest.map((row) => (
          <PlaceRowButton
            key={row.place.key}
            row={row}
            active={active === row.place.key}
            aside={<span className="row-time">{ago(lastActive(row), now)}</span>}
          />
        ))}
      </div>

      <Body />
    </aside>
  );
}

/** The body she runs on: the one thing here that is the operator's. */
function Body() {
  const status = useStore((s) => s.status);
  const watch = useStore((s) => s.watch);
  const [open, setOpen] = useState(false);
  const channels = status?.channels.filter((c) => c.channel !== CHANNEL) ?? [];
  const model = status?.brains.think.split("/").pop();
  const live = watch.is === "open";

  return (
    <div className="body-strip-wrap">
      <button className={`body-strip${open ? " is-on" : ""}`} onClick={() => setOpen(!open)} title="Her body">
        <Icon.Plug />
        <span className="body-strip-text">
          {live
            ? `${channels.length === 0 ? "this console only" : channels.map((c) => c.channel).join(", ")}${model ? ` · ${model}` : ""}`
            : watch.is === "connecting"
              ? "connecting"
              : "not connected"}
        </span>
        <span className={`dot ${live ? "is-live" : "is-down"}`} />
      </button>
      {open ? <BodySheet close={() => setOpen(false)} /> : null}
    </div>
  );
}

function BodySheet({ close }: { close: () => void }) {
  const saved = useStore((s) => s.settings);
  const watch = useStore((s) => s.watch);
  const talk = useStore((s) => s.talk);
  const status = useStore((s) => s.status);
  const [draft, setDraft] = useState(saved);
  const sheet = useRef<HTMLFormElement>(null);

  useEffect(() => {
    const away = (event: MouseEvent) => {
      const target = event.target as Element;
      if (sheet.current && !sheet.current.contains(target) && !target.closest(".body-strip")) close();
    };
    const escape = (event: KeyboardEvent) => event.key === "Escape" && close();
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", escape);
    };
  }, [close]);

  return (
    <form
      ref={sheet}
      className="sheet"
      onSubmit={(event) => {
        event.preventDefault();
        saveSettings(draft);
        close();
      }}
    >
      <div className="sheet-title">Her body</div>
      <p className="sheet-lede">
        What she runs on and how she is reached. This part is yours to look after. Nothing above it is yours to change.
      </p>

      {status ? (
        <dl className="facts-list">
          <dt>Channels</dt>
          <dd>
            {status.channels.length === 0
              ? "none connected"
              : status.channels.map((c) => (
                  <div key={c.channel}>
                    {c.channel}
                    <span className="faint"> · {c.can.join(", ")}</span>
                  </div>
                ))}
          </dd>
          <dt>Thinks on</dt>
          <dd>{status.brains.think}</dd>
          <dt>Ticks on</dt>
          <dd>{status.brains.tick}</dd>
          <dt>Tempo</dt>
          <dd>
            {Number(status.tempo.toFixed(2))}
            <span className="faint">
              {status.tempo === 1 ? " · human pace" : status.tempo === 0 ? " · no waiting" : " · faster than a person"}
            </span>
          </dd>
          <dt>Brain calls</dt>
          <dd>{status.calls_this_hour} this hour</dd>
        </dl>
      ) : null}

      <div className="sheet-rule" />
      <label>
        <span>Daemon</span>
        <input
          value={draft.url}
          onChange={(e) => setDraft({ ...draft, url: e.target.value })}
          spellCheck={false}
          placeholder="ws://127.0.0.1:7437"
        />
      </label>
      <label>
        <span>Token</span>
        <input
          type="password"
          value={draft.token}
          onChange={(e) => setDraft({ ...draft, token: e.target.value })}
          placeholder="only if the daemon has one"
          autoComplete="off"
        />
      </label>
      <label>
        <span>The name she knows you by</span>
        <input value={draft.you} onChange={(e) => setDraft({ ...draft, you: e.target.value })} placeholder="you" />
      </label>
      <div className="sheet-foot">
        <span className="sheet-state">
          {watch.is === "open"
            ? talk.is === "open"
              ? "watching, and able to talk"
              : talk.is === "refused"
                ? "watching only: another console is talking"
                : "watching"
            : watch.is === "connecting"
              ? "connecting"
              : watch.why}
        </span>
        <button type="submit" className="button">
          Reconnect
        </button>
      </div>
    </form>
  );
}
