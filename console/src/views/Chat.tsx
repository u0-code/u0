// A conversation with one ghost.
//
// What you wrote is in bubbles on the right. What she said stands plain on
// the left. Between them, small and grey, are the steps of her mind: how long
// your message waited before she read it, what she thought, when she chose to
// say nothing. The composer at the bottom is where you write, and where the
// model she thinks with can be changed.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Avatar } from "../Avatar";
import { clock } from "../format";
import { Icon } from "../Icon";
import { Popover, useNow } from "../Menu";
import { label, provider, tint, type Local } from "../shell";
import {
  chooseModel,
  mine,
  say,
  show,
  talkFromHere,
  typingToHer,
  useStore,
  wake,
  type Session,
} from "../store";
import { build, type Item, type Step } from "../thread";
import { cap, live, of } from "../words";

export function Chat({ session }: { session: Session }) {
  const timeZone = session.status?.timezone;
  const items = useMemo(() => build(session.entries, timeZone), [session.entries, timeZone]);
  const open = session.watch?.is === "open";

  return (
    <section className="chat">
      {!session.url ? (
        <Idle session={session} />
      ) : !open || !session.loaded ? (
        <Waiting session={session} />
      ) : items.length === 0 ? (
        <Fresh session={session} />
      ) : (
        <Thread session={session} items={items} />
      )}
      <Composer session={session} />
    </section>
  );
}

// --- the thread -----------------------------------------------------------------

function Thread({ session, items }: { session: Session; items: Item[] }) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const now = useNow(1000);
  const timeZone = session.status?.timezone;
  const state = live(session, mine(), now);
  // The newest thing she said carries the time and the small actions.
  const lastHer = useMemo(() => {
    for (let i = items.length - 1; i >= 0; i--) if (items[i]!.is === "her") return items[i]!.key;
    return undefined;
  }, [items]);

  // Stay at the newest thing, unless the reader has scrolled up to look back.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [items, state?.is, session.id]);
  useEffect(() => {
    pinned.current = true;
  }, [session.id]);

  return (
    <div
      className="thread"
      ref={scroller}
      onScroll={(e) => {
        const el = e.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
      }}
    >
      <div className="col">
        {items.map((item) => {
          if (item.is === "day") {
            return (
              <div className="day" key={item.key}>
                {item.label}
              </div>
            );
          }
          if (item.is === "me") {
            return (
              <div className="me" key={item.key} title={clock(item.at, timeZone)}>
                {item.text}
              </div>
            );
          }
          if (item.is === "steps") {
            return (
              <div className="steps" key={item.key}>
                {item.steps.map((step) => (
                  <StepRow key={step.key} step={step} />
                ))}
              </div>
            );
          }
          return (
            <Said
              key={item.key}
              text={item.text}
              at={clock(item.at, timeZone)}
              meta={item.key === lastHer}
              run={item.key === lastHer ? runEndingAt(items, item.key) : ""}
            />
          );
        })}
        {state?.is === "typing" && (
          <div className="live">
            <Avatar seal={session.seal} picture={session.avatar} size="xs" />
            <span>{state.text}</span>
            <span className="dots">
              <i />
              <i />
              <i />
            </span>
          </div>
        )}
        {state?.is === "wait" && (
          <div className="live">
            <Icon name="clock" small />
            <span>{state.text}</span>
          </div>
        )}
        {state?.is === "asleep" && (
          <div className="live">
            <Icon name="moon" small />
            <span>{state.text}</span>
          </div>
        )}
        {state?.is === "taken" && (
          <div className="live">
            <Icon name="alert" small />
            <span>{state.text}</span>
            <button className="link" onClick={() => talkFromHere(session.id)}>
              Talk from here
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

/** Everything she said in the run of messages that ends at `key`. */
function runEndingAt(items: Item[], key: string): string {
  const end = items.findIndex((item) => item.key === key);
  const run: string[] = [];
  for (let i = end; i >= 0; i--) {
    const item = items[i]!;
    if (item.is !== "her") break;
    run.unshift(item.text);
  }
  return run.join("\n");
}

function Said({ text, at, meta, run }: { text: string; at: string; meta: boolean; run: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <>
      <div className="her" title={meta ? undefined : at}>
        {text}
      </div>
      {meta && (
        <div className="meta">
          <span>{at}</span>
          <button
            className="ib"
            aria-label="Copy"
            onClick={() => {
              void navigator.clipboard?.writeText(run).then(() => {
                setCopied(true);
                setTimeout(() => setCopied(false), 1400);
              });
            }}
          >
            <Icon name={copied ? "check" : "copy"} small />
          </button>
        </div>
      )}
    </>
  );
}

function StepRow({ step }: { step: Step }) {
  const [open, setOpen] = useState(false);
  const long = step.more !== undefined || step.detail.length > 46;
  const body = (
    <>
      {step.more !== undefined && <Icon name={open ? "down" : "right"} small />}
      <b>{step.verb}</b>
      {step.delta !== undefined && step.delta !== 0 && (
        <em className={step.delta > 0 ? "up" : "dn"}>{step.delta > 0 ? `+${step.delta}` : `−${-step.delta}`}</em>
      )}
      <span>{step.detail}</span>
      {open && <p>{step.more ?? step.detail}</p>}
    </>
  );
  if (!long) return <div className="step">{body}</div>;
  return (
    <button className={`step can${open ? " open" : ""}`} onClick={() => setOpen(!open)} aria-expanded={open}>
      {body}
    </button>
  );
}

// --- when there is no thread to show -----------------------------------------------

function Idle({ session }: { session: Session }) {
  const doing = useStore((s) => s.busy[session.id]);
  const trouble = useStore((s) => s.trouble);
  const hasModel = useStore((s) => (s.local?.models.length ?? 0) > 0);
  const hasDaemon = useStore((s) => s.local?.ghostd != null);
  const p = of(session);
  return (
    <div className="empty">
      <Avatar seal={session.seal} picture={session.avatar} size="xxl" />
      <h2>{session.name}</h2>
      {!hasModel ? (
        <>
          <p>
            {session.name} has nothing to think with yet. Add a model and {p.sub} can wake up.
          </p>
          <button className="btn primary" onClick={() => show("settings")}>
            Add a model
          </button>
        </>
      ) : !hasDaemon ? (
        <p>
          The <code>ghostd</code> program was not found on this machine. Build it, or point <code>GHOSTD_BIN</code> at
          it.
        </p>
      ) : (
        <>
          <p>
            {session.name} is not running on this machine. Nothing happens in {p.pos} life until {p.sub} {p.is}.
          </p>
          <button className="btn primary" onClick={() => void wake(session.id)} disabled={doing !== undefined}>
            {doing ? `${doing}…` : `Run ${session.name}`}
          </button>
        </>
      )}
      {trouble && <p className="trouble">{trouble}</p>}
    </div>
  );
}

function Waiting({ session }: { session: Session }) {
  const refused = session.watch?.is === "refused" ? session.watch.why : null;
  return (
    <div className="empty">
      <Avatar seal={session.seal} picture={session.avatar} size="xxl" />
      <h2>{session.name}</h2>
      <p>{refused ? cap(refused) + "." : "Connecting…"}</p>
    </div>
  );
}

function Fresh({ session }: { session: Session }) {
  const p = of(session);
  return (
    <div className="empty">
      <Avatar seal={session.seal} picture={session.avatar} size="xxl" />
      <h2>{session.name}</h2>
      <p>
        You have not spoken yet. Write to {p.obj} the way you would to anyone: {p.sub} will see it when {p.sub} next
        look{p.s} at {p.pos} phone, and answer if {p.sub} want{p.s} to.
      </p>
    </div>
  );
}

// --- the composer -------------------------------------------------------------------

function Composer({ session }: { session: Session }) {
  const [text, setText] = useState("");
  const [focus, setFocus] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const lastTyping = useRef(0);
  const can = session.talk?.is === "open";
  const p = of(session);

  // Each ghost keeps her own unsent draft.
  const drafts = useRef(new Map<string, string>());
  useEffect(() => {
    setText(drafts.current.get(session.id) ?? "");
    box.current?.focus();
  }, [session.id]);

  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  }, [text]);

  const send = () => {
    if (!say(text)) return;
    setText("");
    drafts.current.delete(session.id);
  };

  return (
    <div className="cwrap">
      <div className="col">
        <div className={`comp${focus ? " focus" : ""}${session.url ? "" : " off"}`} onClick={() => box.current?.focus()}>
          <textarea
            ref={box}
            rows={1}
            value={text}
            placeholder={session.url ? `Message ${session.name}` : `${session.name} is not running`}
            disabled={!session.url}
            onFocus={() => setFocus(true)}
            onBlur={() => setFocus(false)}
            onChange={(e) => {
              setText(e.target.value);
              drafts.current.set(session.id, e.target.value);
              // She can see that you are still writing, and waits.
              if (Date.now() - lastTyping.current > 3000) {
                lastTyping.current = Date.now();
                typingToHer();
              }
            }}
            onKeyDown={(e) => {
              // Enter sends, except while an input method is still composing.
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                send();
              }
            }}
          />
          <div className="comp-row" onClick={(e) => e.stopPropagation()}>
            <ModelChip session={session} />
            <span className="grow" />
            <button className={`send${can && text.trim() ? " go" : ""}`} onClick={send} disabled={!can || !text.trim()} aria-label="Send">
              <Icon name="up" />
            </button>
          </div>
        </div>
        <div className="hint">
          {session.name} answers in {p.pos} own time.
        </div>
      </div>
    </div>
  );
}

/** The model she thinks with, and the menu that changes it. */
function ModelChip({ session }: { session: Session }) {
  const local = useStore((s) => s.local);
  const doing = useStore((s) => s.busy[session.id]);

  // Without a keeper the window can only say what the daemon reports.
  if (!local || !session.local) {
    const brain = session.status?.brains.think;
    return brain ? (
      <span className="chip still">
        <i />
        {brain}
      </span>
    ) : null;
  }

  const current = pick(local, session.local.model);
  return (
    <Popover
      place="above"
      button={(open, toggle) => (
        <button className={`chip${open ? " open" : ""}`} onClick={toggle} disabled={doing !== undefined}>
          <i style={current ? { background: tint(current.model) } : undefined} />
          {doing ? `${doing}…` : current ? label(current.model) : "No model"}
          <Icon name="down" small />
        </button>
      )}
    >
      {(close) => (
        <div onClick={close}>
          <small>{session.name} thinks with</small>
          {local.models.map(({ model }) => (
            <button
              className="mi"
              key={model.id}
              onClick={() => model.id !== current?.model.id && void chooseModel(session.id, model.id)}
            >
              <i style={{ background: tint(model) }} />
              <span className="mi-t">{label(model)}</span>
              {model.id === current?.model.id ? (
                <Icon name="check" small />
              ) : (
                provider(model) !== label(model) && <span className="mi-r">{provider(model)}</span>
              )}
            </button>
          ))}
          {local.models.length > 0 && <hr />}
          <button className="mi dim" onClick={() => show("settings")}>
            <Icon name="plus" small />
            Add a model
          </button>
        </div>
      )}
    </Popover>
  );
}

function pick(local: Local, id: string | null) {
  return local.models.find((m) => m.model.id === id) ?? local.models[0] ?? null;
}
