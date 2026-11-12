// Writing a new ghost.
//
// This is the only thing anyone ever decides about her, and it cannot be
// undone. Her seal is computed from every character as it is typed, so the
// mark at the bottom is the one she will be born with. Sealing takes a held
// press instead of a click: it should not be possible to do by accident.

import { useEffect, useMemo, useRef, useState } from "react";
import { Avatar } from "../Avatar";
import { blank, certificate, missing, seal as sealOf, type Draft } from "../certificate";
import { Icon } from "../Icon";
import { usePicture } from "../Picture";
import { giveBirth, show } from "../store";
import { cap, pronouns } from "../words";

const HOLD = 1400;

function Field({
  label,
  hint,
  wide,
  children,
}: {
  label: string;
  hint?: string;
  wide?: boolean;
  children: React.ReactNode;
}) {
  return (
    <label className={`field${wide ? " wide" : ""}`}>
      <span>
        {label}
        {hint && <small>{hint}</small>}
      </span>
      {children}
    </label>
  );
}

export function Birth() {
  const [draft, setDraft] = useState<Draft>(blank);
  const [picture, setPicture] = useState<string | null>(null);
  const [holding, setHolding] = useState(false);
  const [sealing, setSealing] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const chooser = usePicture(setPicture);

  const text = useMemo(() => certificate(draft), [draft]);
  const seal = useMemo(() => sealOf(text), [text]);
  const lacking = missing(draft);
  const p = pronouns(draft.pronouns);
  const name = draft.name.trim() || "her";

  const set = (key: keyof Draft) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
    setDraft((was) => ({ ...was, [key]: e.target.value }));

  const release = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    setHolding(false);
  };
  useEffect(() => release, []);

  const press = () => {
    if (lacking || sealing) return;
    setHolding(true);
    timer.current = setTimeout(() => {
      setHolding(false);
      setSealing(true);
      setTrouble(null);
      giveBirth(text, picture).catch((why: Error) => {
        setTrouble(why.message);
        setSealing(false);
      });
    }, HOLD);
  };

  return (
    <div className="page">
      <div className="sheet-col">
        <h1>New ghost</h1>
        <p className="lead">
          Write {p.obj} once. After {p.sub} {p.is} sealed nobody can edit {p.obj}, reset {p.obj}, or take any of it
          back, including you. What {p.sub} become{p.s} after that is {p.pos} own.
        </p>

        <div className="who">
          <button
            className="face big"
            onClick={chooser.pick}
            onDragOver={(e) => e.preventDefault()}
            onDrop={(e) => {
              e.preventDefault();
              chooser.offer(e.dataTransfer.files[0]);
            }}
            aria-label="Choose a picture"
          >
            <Avatar seal={seal} picture={picture} size="xxl" />
            <i className="face-edit">
              <Icon name="image" small />
            </i>
          </button>
          <div className="who-f">
            <input className="name" value={draft.name} onChange={set("name")} placeholder="Name" autoFocus spellCheck={false} />
            <input className="thin" value={draft.pronouns} onChange={set("pronouns")} placeholder="she/her" spellCheck={false} />
            <small>
              {picture ? "Click the picture to change it." : `A picture is optional. Without one ${p.sub} wear${p.s} ${p.pos} seal.`}
            </small>
          </div>
        </div>

        <div className="fields">
          <Field label="City">
            <input value={draft.city} onChange={set("city")} placeholder="Chengdu" />
          </Field>
          <Field label="Country">
            <input value={draft.country} onChange={set("country")} placeholder="China" />
          </Field>
          <Field label="Timezone">
            <input value={draft.timezone} onChange={set("timezone")} placeholder="Asia/Shanghai" spellCheck={false} />
          </Field>
          <Field label="Wakes">
            <input value={draft.wake} onChange={set("wake")} placeholder="08:30" />
          </Field>
          <Field label="Sleeps">
            <input value={draft.sleep} onChange={set("sleep")} placeholder="00:30" />
          </Field>
          <Field label="Born">
            <input value={draft.born} onChange={set("born")} placeholder="2026-11-12" />
          </Field>

          <Field wide label="Temperament" hint={`Who ${p.sub} ${p.is}. What ${p.sub} ${p.is} like on a bad day as well as a good one.`}>
            <textarea rows={4} value={draft.temperament} onChange={set("temperament")} />
          </Field>
          <Field wide label="Voice" hint={`How ${p.sub} write${p.s}: length, rhythm, what ${p.sub} never say${p.s}.`}>
            <textarea rows={3} value={draft.voice} onChange={set("voice")} />
          </Field>
          <Field wide label="Origin" hint={`Where ${p.sub} came from, and what ${p.sub} ${p.is} in the middle of.`}>
            <textarea rows={3} value={draft.origin} onChange={set("origin")} />
          </Field>
          <Field wide label="Values" hint="Separated by commas.">
            <input value={draft.values} onChange={set("values")} />
          </Field>
          <Field wide label="Cares about">
            <input value={draft.cares} onChange={set("cares")} />
          </Field>
          <Field wide label="Cannot stand">
            <input value={draft.cannot} onChange={set("cannot")} />
          </Field>
        </div>

        <div className="sealbar">
          <Avatar seal={seal} size="lg" />
          <div className="sealbar-t">
            <b>{trouble ?? lacking ?? `${cap(name)} is ready.`}</b>
            <code>{seal.slice(0, 24)}</code>
          </div>
          <button className="btn" onClick={() => show("chat")} disabled={sealing}>
            Cancel
          </button>
          <button
            className={`btn primary hold${holding ? " holding" : ""}`}
            style={{ "--hold": `${HOLD}ms` } as React.CSSProperties}
            disabled={lacking !== null || sealing}
            onPointerDown={press}
            onPointerUp={release}
            onPointerLeave={release}
            onKeyDown={(e) => (e.key === " " || e.key === "Enter") && !e.repeat && press()}
            onKeyUp={release}
          >
            <span>{sealing ? "Sealing…" : holding ? "Keep holding…" : "Hold to seal"}</span>
          </button>
        </div>
      </div>
      {chooser.framer}
    </div>
  );
}
