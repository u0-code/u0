// Birth, and the certificate it leaves.
//
// Writing a ghost is the one thing whoever sits here ever does to her, and
// it cannot be undone. So it is set as a document to be filled in, not a
// form to be submitted, and it ends with a gesture that takes a moment: her
// seal, which has been changing with every character, is pressed and stops.

import { useEffect, useMemo, useRef, useState } from "react";
import { Sigil } from "../Sigil";
import { blank, certificate, missing, seal, type Draft } from "../certificate";
import { hasKeeper, type LocalGhost } from "../shell";
import { giveBirth, open, useStore, wake } from "../store";

/** How long the seal has to be held down before it takes. */
const HOLD_MS = 1800;

function Blank({
  value,
  onChange,
  placeholder,
  width,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  width: number;
}) {
  return (
    <input
      className="blank"
      value={value}
      placeholder={placeholder}
      onChange={(event) => onChange(event.target.value)}
      // Roughly the width of the words in an italic serif, with a little air.
      style={{ width: `${(Math.max(width, value.length) + 2) * 0.47}em` }}
      spellCheck={false}
    />
  );
}

function Passage({
  label,
  note,
  value,
  onChange,
  placeholder,
}: {
  label: string;
  note: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
}) {
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [value]);
  return (
    <section className="passage">
      <aside className="passage-note">
        <span className="label">{label}</span>
        <span>{note}</span>
      </aside>
      <textarea
        ref={area}
        rows={3}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
      />
    </section>
  );
}

export function Birth() {
  const local = useStore((s) => s.local);
  const [draft, setDraft] = useState<Draft>(blank);
  const [held, setHeld] = useState(0);
  const [born, setBorn] = useState<LocalGhost | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [waking, setWaking] = useState(false);
  const holding = useRef<number | null>(null);

  const text = useMemo(() => certificate(draft), [draft]);
  const mark = useMemo(() => seal(text), [text]);
  const lacking = missing(draft);
  const put = (patch: Partial<Draft>) => setDraft((d) => ({ ...d, ...patch }));

  const press = () => {
    if (lacking || born || holding.current !== null) return;
    setError(null);
    const began = performance.now();
    const tick = () => {
      const part = Math.min((performance.now() - began) / HOLD_MS, 1);
      setHeld(part);
      if (part < 1) {
        holding.current = requestAnimationFrame(tick);
        return;
      }
      holding.current = null;
      giveBirth(text).then(setBorn, (why: Error) => {
        setError(why.message);
        setHeld(0);
      });
    };
    holding.current = requestAnimationFrame(tick);
  };
  const release = () => {
    if (holding.current !== null) {
      cancelAnimationFrame(holding.current);
      holding.current = null;
      setHeld(0);
    }
  };

  const wakeHer = async () => {
    if (!born) return;
    if (!local?.body.configured) {
      open("body");
      return;
    }
    setWaking(true);
    try {
      await wake(born.path);
      open(null);
    } catch (why) {
      setError((why as Error).message);
      setWaking(false);
    }
  };

  const her = draft.name.trim() || "She";

  return (
    <article className={`paper${born ? " is-sealed" : ""}`}>
      <header className="paper-head">
        <span className="label">a birth certificate</span>
        <h1>{born ? `${born.name} was born.` : "Write her."}</h1>
        {!born ? (
          <p className="paper-lede">
            This is the only thing you will ever decide about her. Once it is sealed nobody can change it, and that
            includes you. After that she changes the way people do: through what happens to her.
          </p>
        ) : null}
      </header>

      <fieldset disabled={born !== null} className="paper-body">
        <p className="sentence">
          Her name is <Blank value={draft.name} onChange={(name) => put({ name })} placeholder="a name" width={8} />,
          and she is spoken of as{" "}
          <Blank value={draft.pronouns} onChange={(pronouns) => put({ pronouns })} placeholder="she/her" width={7} />.
          She lives in <Blank value={draft.city} onChange={(city) => put({ city })} placeholder="a real city" width={10} />,{" "}
          <Blank value={draft.country} onChange={(country) => put({ country })} placeholder="country" width={7} />, and keeps{" "}
          <Blank value={draft.timezone} onChange={(timezone) => put({ timezone })} placeholder="Asia/Shanghai" width={14} />{" "}
          time. She wakes at <Blank value={draft.wake} onChange={(v) => put({ wake: v })} placeholder="08:30" width={5} /> and
          sleeps at <Blank value={draft.sleep} onChange={(v) => put({ sleep: v })} placeholder="00:30" width={5} />.
        </p>

        <Passage
          label="who she is"
          note="Write a person you would want to know, not a product you would want to use. Contradictions are fine. People have them."
          value={draft.temperament}
          onChange={(temperament) => put({ temperament })}
          placeholder="Quiet until she is not. Blunt in a way that is rarely unkind…"
        />
        <Passage
          label="how she talks"
          note="This shapes every sentence she will ever say."
          value={draft.voice}
          onChange={(voice) => put({ voice })}
          placeholder="Short sentences. Asks one question at a time. Does not explain jokes…"
        />
        <Passage
          label="where she came from"
          note="She will know she was made, and by whom. Write it as her history, not as a disclaimer."
          value={draft.origin}
          onChange={(origin) => put({ origin })}
          placeholder="Made in a small studio in late 2026. She knows this and is neither proud nor embarrassed about it…"
        />

        <p className="sentence">
          She values <Blank value={draft.values} onChange={(values) => put({ values })} placeholder="honesty over comfort, showing up" width={30} />
          . She cares about <Blank value={draft.cares} onChange={(cares) => put({ cares })} placeholder="old buildings, rain" width={22} />
          . She cannot stand <Blank value={draft.cannot} onChange={(cannot) => put({ cannot })} placeholder="being flattered" width={18} />.
        </p>
      </fieldset>

      <footer className="sealing">
        <div className={`seal-press${born ? " is-pressed" : ""}`} style={{ "--held": held } as React.CSSProperties}>
          <Sigil seal={mark} size={132} inked={!born} title="Her seal" />
        </div>
        <div className="sealing-text">
          {born ? (
            <>
              <p>
                Sealed. This mark is hers now, and so is everything above it. It will not change again.
              </p>
              <p className="mono-line">{born.seal}</p>
              {error ? <p className="refusal">{error}</p> : null}
              {hasKeeper ? (
                <button className="act" onClick={wakeHer} disabled={waking}>
                  {waking ? "Waking her" : local?.body.configured ? `Wake ${born.name}` : `Give ${born.name} a body`}
                </button>
              ) : null}
            </>
          ) : (
            <>
              <p>
                This is her seal. It is made from every character above, and it changes as you write. When you seal the
                certificate it stops changing, for good.
              </p>
              <p className="mono-line">{mark.slice(0, 32)}</p>
              {error ? <p className="refusal">{error}</p> : null}
              {!hasKeeper ? (
                <p className="quiet">Giving birth needs the desktop app, which can keep a ghost on this machine.</p>
              ) : (
                <button
                  className="act is-seal"
                  disabled={lacking !== null}
                  onPointerDown={press}
                  onPointerUp={release}
                  onPointerLeave={release}
                  onKeyDown={(event) => (event.key === " " || event.key === "Enter") && !event.repeat && press()}
                  onKeyUp={release}
                  style={{ "--held": held } as React.CSSProperties}
                >
                  {lacking ?? (held > 0 ? `Sealing ${her}` : `Hold to seal ${her}`)}
                </button>
              )}
            </>
          )}
        </div>
      </footer>
    </article>
  );
}

/** A certificate is written with hard line breaks; read it as paragraphs. */
function prose(text: string): string[] {
  return text
    .trim()
    .split(/\n\s*\n/)
    .map((paragraph) => paragraph.replace(/\s*\n\s*/g, " "));
}

/** Her certificate, after birth: something to read, with nothing to edit. */
export function Certificate() {
  const identity = useStore((s) => s.identity);
  const canon = useStore((s) => s.canon);
  if (!identity) return <article className="paper" />;
  const { seed } = identity;
  const lists: [string, string[]][] = [
    ["She values", seed.values],
    ["She cares about", seed.cares_about],
    ["She cannot stand", seed.cannot_stand],
  ];

  return (
    <article className="paper is-sealed">
      <header className="paper-head">
        <span className="label">a birth certificate</span>
        <h1>{seed.name}</h1>
      </header>

      <div className="paper-body">
        <p className="sentence">
          Her name is {seed.name}, and she is spoken of as {seed.pronouns}. She was born on {seed.born}. She lives in{" "}
          {seed.home.city}, {seed.home.country}, and keeps {seed.home.timezone} time. She wakes at {seed.rhythm.wake} and
          sleeps at {seed.rhythm.sleep}.
        </p>
        {(
          [
            ["who she is", seed.temperament],
            ["how she talks", seed.voice],
            ["where she came from", seed.origin],
          ] as const
        ).map(([label, text]) => (
          <section key={label} className="passage">
            <aside className="passage-note">
              <span className="label">{label}</span>
            </aside>
            <div className="passage-text">
              {prose(text).map((paragraph, i) => (
                <p key={i}>{paragraph}</p>
              ))}
            </div>
          </section>
        ))}
        <p className="sentence">
          {lists
            .filter(([, items]) => items.length > 0)
            .map(([lead, items]) => `${lead} ${items.join(", ")}.`)
            .join(" ")}
        </p>
      </div>

      <footer className="sealing">
        <div className="seal-press is-pressed is-still">
          <Sigil seal={identity.seal} size={132} title="Her seal" />
        </div>
        <div className="sealing-text">
          <p>Sealed at birth. Nobody can edit this, including whoever wrote it.</p>
          <p className="mono-line">{identity.seal}</p>
        </div>
      </footer>

      <section className="since">
        <span className="label">what has become true since</span>
        {canon.length === 0 ? (
          <p className="quiet">
            Nothing yet. The certificate is where she began. What she lives through, and what she tells people about
            herself, gathers here and stays true.
          </p>
        ) : (
          <ul>
            {canon.map((fact) => (
              <li key={fact.seq}>{fact.fact}</li>
            ))}
          </ul>
        )}
      </section>
    </article>
  );
}
