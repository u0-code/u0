// The frame of the window: the title bar, and the list of ghosts down the
// left. The list is the way around. Collapsed, it is a column of faces.

import { useState } from "react";
import { Avatar } from "../Avatar";
import { Icon } from "../Icon";
import { Popover, useNow } from "../Menu";
import { usePicture } from "../Picture";
import {
  rest,
  select,
  setAvatar,
  show,
  togglePanel,
  toggleSidebar,
  unread,
  useActive,
  useStore,
  wake,
  type Session,
} from "../store";
import { line, presence, stamp, whereabouts } from "../words";

export function TitleBar() {
  const session = useActive();
  const view = useStore((s) => s.view);
  const sidebar = useStore((s) => s.sidebar);
  const panel = useStore((s) => s.panel);
  const now = useNow(5000);
  const picture = usePicture((data) => session && void setAvatar(session.id, data));
  const chatting = view === "chat" && session;

  return (
    <header className="bar" data-tauri-drag-region>
      <button className={`ib${sidebar ? "" : " on"}`} onClick={toggleSidebar} aria-label="Toggle the list of ghosts">
        <Icon name="side" />
      </button>
      <div className="bar-title" data-tauri-drag-region>
        {chatting ? (
          <>
            <Avatar seal={session.seal} picture={session.avatar} size="xs" />
            <span>{session.name}</span>
            <small>{whereabouts(session, now)}</small>
          </>
        ) : (
          <span>{view === "birth" ? "New ghost" : view === "settings" ? "Settings" : view === "certificate" ? "Birth certificate" : "Ghost"}</span>
        )}
      </div>
      <div className="bar-r">
        {chatting && (
          <>
            <Popover
              align="end"
              button={(open, toggle) => (
                <button className={`ib${open ? " on" : ""}`} onClick={toggle} aria-label="More">
                  <Icon name="more" />
                </button>
              )}
            >
              {(close) => (
                <div onClick={close}>
                  {session.local && (
                    <>
                      {session.url ? (
                        <button className="mi" onClick={() => void rest(session.id)}>
                          <Icon name="power" small />
                          Stop running {session.name}
                        </button>
                      ) : (
                        <button className="mi" onClick={() => void wake(session.id)}>
                          <Icon name="power" small />
                          Run {session.name}
                        </button>
                      )}
                      <button className="mi" onClick={picture.pick}>
                        <Icon name="image" small />
                        {session.avatar ? "Change picture" : "Give a picture"}
                      </button>
                    </>
                  )}
                  <button className="mi" onClick={() => show("certificate")}>
                    <Icon name="scroll" small />
                    Birth certificate
                  </button>
                </div>
              )}
            </Popover>
            <button className={`ib${panel ? " on" : ""}`} onClick={togglePanel} aria-label="Toggle the side panel">
              <Icon name="panel" />
            </button>
          </>
        )}
      </div>
      {picture.framer}
    </header>
  );
}

function lastAt(session: Session): string | undefined {
  return session.entries[session.entries.length - 1]?.at;
}

export function Sidebar() {
  const sessions = useStore((s) => s.sessions);
  const active = useStore((s) => s.active);
  const view = useStore((s) => s.view);
  const seen = useStore((s) => s.seen);
  const busy = useStore((s) => s.busy);
  const canBirth = useStore((s) => s.local !== null);
  const now = useNow(1000);
  const [query, setQuery] = useState<string | null>(null);

  const shown = query ? sessions.filter((s) => s.name.toLowerCase().includes(query.trim().toLowerCase())) : sessions;

  return (
    <aside className="side">
      <div className="side-h">
        {query === null ? (
          <>
            <b>Ghost</b>
            <button className="ib" onClick={() => setQuery("")} aria-label="Find a ghost">
              <Icon name="search" />
            </button>
          </>
        ) : (
          <label className="find">
            <Icon name="search" small />
            <input
              autoFocus
              value={query}
              placeholder="Find a ghost"
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") setQuery(null);
                if (e.key === "Enter" && shown[0]) {
                  select(shown[0].id);
                  setQuery(null);
                }
              }}
              onBlur={() => query === "" && setQuery(null)}
            />
          </label>
        )}
      </div>
      {canBirth && (
        <button className={`nav${view === "birth" ? " sel" : ""}`} onClick={() => show("birth")}>
          <Icon name="pen" />
          <span>New ghost</span>
        </button>
      )}
      <div className="sec">
        <span>Ghosts</span>
      </div>
      <div className="rows">
        {shown.map((session) => {
          const news = unread(session, seen);
          return (
            <button
              key={session.id}
              className={`row${view === "chat" && session.id === active ? " sel" : ""}`}
              onClick={() => select(session.id)}
            >
              <Avatar seal={session.seal} picture={session.avatar} state={presence(session)} />
              <span className="row-t">
                <b>{session.name}</b>
                <span>{line(session, busy[session.id], now)}</span>
              </span>
              <span className="row-m">
                {stamp(lastAt(session), session.status?.timezone, now)}
                {news > 0 && <i className="un">{news > 99 ? "99+" : news}</i>}
              </span>
            </button>
          );
        })}
        {shown.length === 0 && <p className="none">{query ? "Nobody by that name." : "Nobody lives here yet."}</p>}
      </div>
      <div className="side-f">
        <button className={`nav${view === "settings" ? " sel" : ""}`} onClick={() => show("settings")}>
          <Icon name="sliders" />
          <span>Settings</span>
        </button>
      </div>
    </aside>
  );
}

/** The list, collapsed to a column of faces. */
export function Rail() {
  const sessions = useStore((s) => s.sessions);
  const active = useStore((s) => s.active);
  const view = useStore((s) => s.view);
  const seen = useStore((s) => s.seen);
  const busy = useStore((s) => s.busy);
  const canBirth = useStore((s) => s.local !== null);
  const now = useNow(1000);

  return (
    <aside className="rail">
      {sessions.map((session) => (
        <button
          key={session.id}
          className={`slot${view === "chat" && session.id === active ? " sel" : ""}`}
          onClick={() => select(session.id)}
          data-tip={`${session.name} · ${line(session, busy[session.id], now)}`}
        >
          <Avatar seal={session.seal} picture={session.avatar} state={presence(session)} />
          {unread(session, seen) > 0 && <i className="dot" />}
        </button>
      ))}
      {canBirth && (
        <button className="slot" onClick={() => show("birth")} data-tip="New ghost">
          <span className="add">
            <Icon name="plus" />
          </span>
        </button>
      )}
      <button className={`slot foot${view === "settings" ? " sel" : ""}`} onClick={() => show("settings")} data-tip="Settings">
        <span className="add plain">
          <Icon name="sliders" />
        </span>
      </button>
    </aside>
  );
}
