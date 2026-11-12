// Who she is and how she is, beside the conversation: her moment, her day,
// what there is between her and whoever is at this screen, and the way to
// her birth certificate.

import { Avatar, Mark, hue } from "../Avatar";
import { ago, clock } from "../format";
import { Icon } from "../Icon";
import { useNow } from "../Menu";
import { usePicture } from "../Picture";
import { setAvatar, show, useStore, type Session } from "../store";
import { cap, moment, of, presence, standing } from "../words";

/** The day of her life it is: 1 on the day she was born. */
function dayOfLife(born: string | undefined, now: number): number | null {
  if (!born) return null;
  const days = Math.floor((now - new Date(`${born}T00:00:00`).getTime()) / 86400000) + 1;
  return days >= 1 ? days : null;
}

export function About({ session }: { session: Session }) {
  const now = useNow(15000);
  const p = of(session);
  const status = session.status;
  const state = presence(session);
  const picture = usePicture((data) => void setAvatar(session.id, data));

  const born = session.identity?.seed.born ?? session.local?.born;
  const lived = dayOfLife(born, now);
  const about = [status?.pronouns ?? session.local?.pronouns, status?.city ?? session.local?.city, lived ? `day ${lived}` : null]
    .filter(Boolean)
    .join(" · ");

  const you = useStore((s) => s.settings.you).trim().toLowerCase() || "you";
  const me = session.people.find((row) => row.person.channel === "console" && row.person.id === you);
  const warmth = me?.standing.warmth ?? 0;
  const reach = Math.min(50, (Math.abs(warmth) / 20) * 50);

  const blocks = status?.today?.blocks ?? [];
  const current = status?.moment.phase === "awake" ? status.moment.doing?.index : undefined;
  const sleep = session.identity?.seed.rhythm.sleep;

  return (
    <aside className="about">
      {session.local ? (
        <button
          className="face"
          onClick={picture.pick}
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault();
            picture.offer(e.dataTransfer.files[0]);
          }}
          aria-label={`Change ${session.name}'s picture`}
        >
          <Avatar seal={session.seal} picture={session.avatar} size="xl" state={state} />
          <i className="face-edit">
            <Icon name="image" small />
          </i>
        </button>
      ) : (
        <Avatar seal={session.seal} picture={session.avatar} size="xl" state={state} />
      )}
      <h2>{session.name}</h2>
      <div className="sub">{about}</div>

      {status && (
        <div className="now">
          <i className={state} />
          <span>{moment(session)}</span>
        </div>
      )}

      {blocks.length > 0 && (
        <>
          <div className="lab">{cap(p.pos)} day</div>
          <div className="tl">
            {blocks.map((block) => (
              <div
                key={block.index}
                className={block.index === current ? "cur" : new Date(block.to).getTime() <= now ? "past" : ""}
              >
                <time>{clock(block.from, status?.timezone)}</time>
                <span>{block.what}</span>
              </div>
            ))}
            {sleep && (
              <div className={status?.moment.phase === "asleep" ? "cur" : ""}>
                <time>{sleep}</time>
                <span>sleep</span>
              </div>
            )}
          </div>
        </>
      )}

      {status && (
        <>
          <div className="lab">Between you</div>
          <div className="kv">
            <span>{cap(p.sub)} see{p.s} you as</span>
            <b>{me ? standing(me.standing.stage, p) : "nobody yet"}</b>
            {me && (
              <>
                <span>First heard from you</span>
                <b>{ago(me.person.first_at, now)}</b>
              </>
            )}
            <div className="warm" title={`warmth ${warmth > 0 ? "+" : ""}${warmth}`}>
              {warmth !== 0 && (
                <i className={warmth < 0 ? "cold" : ""} style={warmth > 0 ? { left: "50%", width: `${reach}%` } : { right: "50%", width: `${reach}%` }} />
              )}
              <u />
            </div>
          </div>
        </>
      )}

      {me && me.notes.length > 0 && (
        <>
          <div className="lab">{cap(p.sub)} remember{p.s}</div>
          {me.notes.slice(-4).map((note) => (
            <div className="note" key={note.seq}>
              {note.text}
            </div>
          ))}
        </>
      )}

      <div className="gap" />
      <button className="linkrow" onClick={() => show("certificate")}>
        <span>Birth certificate</span>
        <Icon name="right" small />
      </button>
      {picture.framer}
    </aside>
  );
}

/** Her birth certificate, as it was written and sealed. Read only, for ever. */
export function Certificate({ session }: { session: Session }) {
  const seed = session.identity?.seed;
  const seal = session.identity?.seal ?? session.seal;
  const p = of(session);
  return (
    <div className="page">
      <div className="sheet-col">
        <button className="back" onClick={() => show("chat")}>
          <Icon name="left" small />
          {session.name}
        </button>
        <div className="cert-head">
          <span className="sealmark" style={{ "--h": hue(seal) } as React.CSSProperties}>
            <Mark seal={seal} />
          </span>
          <div>
            <h1>{session.name}</h1>
            <p>
              Written once and sealed. Nobody can change {p.obj} now, including whoever wrote this.
            </p>
            <code title={seal}>{seal.slice(0, 16)}…{seal.slice(-8)}</code>
          </div>
        </div>
        {!seed ? (
          <p className="quiet">{cap(p.pos)} certificate can be read while {p.sub} {p.is} running.</p>
        ) : (
          <dl className="cert">
            <dt>Pronouns</dt>
            <dd>{seed.pronouns}</dd>
            <dt>Born</dt>
            <dd>{seed.born}</dd>
            <dt>Lives in</dt>
            <dd>
              {seed.home.city}
              {seed.home.country ? `, ${seed.home.country}` : ""} <span className="quiet">({seed.home.timezone})</span>
            </dd>
            <dt>Rhythm</dt>
            <dd>
              wakes {seed.rhythm.wake}, sleeps {seed.rhythm.sleep}
            </dd>
            <dt>Temperament</dt>
            <dd className="prose">{seed.temperament}</dd>
            <dt>Voice</dt>
            <dd className="prose">{seed.voice}</dd>
            <dt>Origin</dt>
            <dd className="prose">{seed.origin}</dd>
            {seed.values.length > 0 && (
              <>
                <dt>Values</dt>
                <dd>{seed.values.join(", ")}</dd>
              </>
            )}
            {seed.cares_about.length > 0 && (
              <>
                <dt>Cares about</dt>
                <dd>{seed.cares_about.join(", ")}</dd>
              </>
            )}
            {seed.cannot_stand.length > 0 && (
              <>
                <dt>Cannot stand</dt>
                <dd>{seed.cannot_stand.join(", ")}</dd>
              </>
            )}
          </dl>
        )}
      </div>
    </div>
  );
}
