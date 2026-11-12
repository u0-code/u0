// Her body, the ghosts on this machine, and what to show when nobody is awake.
//
// The body is the part of all this that belongs to whoever sits here: what
// she thinks with, how she is reached, whether she is running. It is kept
// out of the way because, when it works, it should not be on anyone's mind.

import { useEffect, useState } from "react";
import { Sigil } from "../Sigil";
import { hasKeeper, type Body as BodyConfig } from "../shell";
import { open, refreshLocal, rest, saveSettings, setBody, useStore, wake } from "../store";

const KINDS: { kind: BodyConfig["kind"]; name: string; note: string }[] = [
  { kind: "anthropic", name: "Anthropic", note: "The Messages API, with a key." },
  { kind: "openai", name: "OpenAI-compatible", note: "OpenAI, DeepSeek, OpenRouter, Ollama, and anything shaped like them." },
  { kind: "claude-code", name: "Claude Code", note: "Through the claude program you are logged in to. For your own experiments." },
  { kind: "codex", name: "Codex", note: "Through the codex program you are logged in to. For your own experiments." },
];

const TEMPOS: [number, string][] = [
  [1, "a person's pace"],
  [0.3, "three times faster"],
  [0.1, "ten times faster"],
  [0, "no waiting at all"],
];

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="field">
      <span className="label">{label}</span>
      {children}
    </label>
  );
}

export function Body() {
  const local = useStore((s) => s.local);
  const status = useStore((s) => s.status);
  const ghost = useStore((s) => s.ghost);
  const watch = useStore((s) => s.watch);
  const talk = useStore((s) => s.talk);
  const saved = useStore((s) => s.settings);
  const [settings, setSettings] = useState(saved);
  const [body, setDraft] = useState<BodyConfig | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Start from what is stored, once it is known.
  useEffect(() => {
    if (body || !local) return;
    setDraft(
      local.body.body ?? {
        kind: "anthropic",
        model: "claude-sonnet-5",
        base_url: null,
        api_key_env: null,
        json_mode: false,
        tempo: 1,
      },
    );
  }, [local, body]);

  const run = async (what: string, work: () => Promise<void>) => {
    setBusy(what);
    setError(null);
    try {
      await work();
    } catch (why) {
      setError((why as Error).message);
    }
    setBusy(null);
  };

  const name = ghost?.name ?? "She";
  const awake = watch.is === "open";
  const here = local?.ghosts.find((g) => g.seal === ghost?.seal) ?? local?.ghosts.find((g) => g.path === local.running?.path);
  const keyed = body?.kind === "anthropic" || body?.kind === "openai";

  return (
    <article className="paper is-plain">
      <header className="paper-head">
        <span className="label">her body</span>
        <h1>What she runs on</h1>
        <p className="paper-lede">
          A body can be replaced without her becoming someone else. This part is yours to look after. Nothing about who
          she is can be changed from here, or from anywhere.
        </p>
      </header>

      {hasKeeper && local ? (
        <section className="part">
          <span className="label">running</span>
          <p>
            {awake
              ? local.running
                ? `${name} is running on this machine.`
                : `${name} is running somewhere this window did not start, at ${saved.url}.`
              : "Nobody is running."}
          </p>
          <div className="acts">
            {awake && local.running ? (
              <button className="act" disabled={busy !== null} onClick={() => run("rest", rest)}>
                {busy === "rest" ? "Stopping" : `Let ${name} rest`}
              </button>
            ) : null}
            {!awake && here ? (
              <button className="act" disabled={busy !== null} onClick={() => run("wake", () => wake(here.path))}>
                {busy === "wake" ? "Waking her" : `Wake ${here.name}`}
              </button>
            ) : null}
            <button className="act is-quiet" onClick={() => open("ghosts")}>
              Everyone on this machine
            </button>
          </div>
          {awake && local.running ? (
            <p className="quiet">While she is stopped she does not live. Her journal records the gap.</p>
          ) : null}
        </section>
      ) : null}

      {hasKeeper && local && body ? (
        <section className="part">
          <span className="label">what she thinks with</span>
          {local.body.by_hand ? (
            <p className="quiet">
              Her body is set by a ghostd.toml written by hand in {local.home}. Saving here replaces it.
            </p>
          ) : null}
          <div className="choices">
            {KINDS.map((k) => (
              <button
                key={k.kind}
                className={`choice${body.kind === k.kind ? " is-on" : ""}`}
                onClick={() => setDraft({ ...body, kind: k.kind })}
              >
                <span>{k.name}</span>
                <span className="choice-note">{k.note}</span>
              </button>
            ))}
          </div>
          <div className="fields">
            <Field label={keyed ? "model" : "model, if not the default"}>
              <input value={body.model} onChange={(e) => setDraft({ ...body, model: e.target.value })} spellCheck={false} />
            </Field>
            {body.kind === "openai" ? (
              <Field label="address">
                <input
                  value={body.base_url ?? ""}
                  onChange={(e) => setDraft({ ...body, base_url: e.target.value })}
                  placeholder="https://api.openai.com/v1"
                  spellCheck={false}
                />
              </Field>
            ) : null}
            {keyed ? (
              <>
                <Field label="key">
                  <input
                    type="password"
                    value={key}
                    onChange={(e) => setKey(e.target.value)}
                    placeholder={local.body.has_key ? "kept on this machine; type to replace" : "kept on this machine only"}
                    autoComplete="off"
                  />
                </Field>
                <Field label="or the name of a variable holding it">
                  <input
                    value={body.api_key_env ?? ""}
                    onChange={(e) => setDraft({ ...body, api_key_env: e.target.value })}
                    placeholder={body.kind === "anthropic" ? "ANTHROPIC_API_KEY" : "OPENAI_API_KEY"}
                    spellCheck={false}
                  />
                </Field>
              </>
            ) : null}
            <Field label="her pace">
              <select value={body.tempo} onChange={(e) => setDraft({ ...body, tempo: Number(e.target.value) })}>
                {TEMPOS.map(([value, words]) => (
                  <option key={value} value={value}>
                    {words}
                  </option>
                ))}
                {TEMPOS.some(([value]) => value === body.tempo) ? null : <option value={body.tempo}>{body.tempo}</option>}
              </select>
            </Field>
          </div>
          <div className="acts">
            <button
              className="act"
              disabled={busy !== null}
              onClick={() =>
                run("body", async () => {
                  await setBody({ ...body, json_mode: body.kind === "openai", api_key: key || null });
                  setKey("");
                })
              }
            >
              {busy === "body" ? "Saving" : "Save her body"}
            </button>
            <span className="quiet">A change takes effect the next time she wakes.</span>
          </div>
        </section>
      ) : null}

      {error ? <p className="refusal">{error}</p> : null}

      {status ? (
        <section className="part">
          <span className="label">right now</span>
          <dl className="ledger">
            <dt>reached through</dt>
            <dd>
              {status.channels.length === 0
                ? "nothing"
                : status.channels.map((c) => `${c.channel} (${c.can.join(", ")})`).join("; ")}
            </dd>
            <dt>thinks on</dt>
            <dd>{status.brains.think}</dd>
            <dt>ticks on</dt>
            <dd>{status.brains.tick}</dd>
            <dt>pace</dt>
            <dd>{Number(status.tempo.toFixed(2))}</dd>
            <dt>brain calls</dt>
            <dd>{status.calls_this_hour} in the last hour</dd>
            <dt>journal</dt>
            <dd>{status.journal} entries</dd>
          </dl>
        </section>
      ) : null}

      <section className="part">
        <span className="label">this window</span>
        <div className="fields">
          <Field label="where she is running">
            <input value={settings.url} onChange={(e) => setSettings({ ...settings, url: e.target.value })} spellCheck={false} />
          </Field>
          <Field label="token, if that daemon asks for one">
            <input
              type="password"
              value={settings.token}
              onChange={(e) => setSettings({ ...settings, token: e.target.value })}
              autoComplete="off"
            />
          </Field>
          <Field label="the name she knows you by">
            <input value={settings.you} onChange={(e) => setSettings({ ...settings, you: e.target.value })} />
          </Field>
          <Field label="the light in this window">
            <select
              value={settings.light}
              onChange={(e) => setSettings({ ...settings, light: e.target.value as "hers" | "system" })}
            >
              <option value="hers">follows her day: light while she is awake, dark while she sleeps</option>
              <option value="system">follows this computer</option>
            </select>
          </Field>
        </div>
        <div className="acts">
          <button className="act" onClick={() => saveSettings(settings)}>
            Save
          </button>
          <span className="quiet">
            {watch.is === "open"
              ? talk.is === "open"
                ? "Connected. You can read and write."
                : "Connected. Another window is talking to her, so this one can only read."
              : watch.is === "connecting"
                ? "Connecting."
                : watch.why}
          </span>
        </div>
      </section>
    </article>
  );
}

/** Every ghost that lives on this machine. */
export function Ghosts() {
  const local = useStore((s) => s.local);
  const ghost = useStore((s) => s.ghost);
  const awake = useStore((s) => s.watch.is === "open");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    void refreshLocal();
  }, []);

  if (!hasKeeper) {
    return (
      <article className="paper is-plain">
        <header className="paper-head">
          <span className="label">on this machine</span>
          <h1>Nobody can be kept here</h1>
          <p className="paper-lede">
            This is the web console. It can watch a ghost that is running somewhere and talk to her. Keeping one on a
            machine, and giving birth, takes the desktop app.
          </p>
        </header>
      </article>
    );
  }

  const ghosts = local?.ghosts ?? [];
  return (
    <article className="paper is-plain">
      <header className="paper-head">
        <span className="label">on this machine</span>
        <h1>{ghosts.length === 0 ? "Nobody lives here yet" : "Who lives here"}</h1>
      </header>
      <section className="part">
        {ghosts.map((g) => {
          const running = awake && g.seal === ghost?.seal;
          return (
            <div key={g.path} className="resident">
              <Sigil seal={g.seal} size={44} />
              <div className="resident-text">
                <span className="resident-name">{g.name}</span>
                <span className="label">
                  {g.city}, {g.country} · born {g.born}
                </span>
              </div>
              {running ? (
                <span className="label">awake</span>
              ) : (
                <button
                  className="act is-quiet"
                  disabled={busy !== null}
                  onClick={async () => {
                    setBusy(g.path);
                    setError(null);
                    try {
                      if (!local?.body.configured) {
                        open("body");
                        return;
                      }
                      await wake(g.path);
                      open(null);
                    } catch (why) {
                      setError((why as Error).message);
                    } finally {
                      setBusy(null);
                    }
                  }}
                >
                  {busy === g.path ? "Waking" : "Wake"}
                </button>
              )}
            </div>
          );
        })}
        {error ? <p className="refusal">{error}</p> : null}
        <div className="acts">
          <button className="act" onClick={() => open("birth")}>
            Give birth to {ghosts.length === 0 ? "someone" : "someone else"}
          </button>
        </div>
      </section>
    </article>
  );
}

/** What the window shows when no daemon answers. */
export function Nobody() {
  const watch = useStore((s) => s.watch);
  const url = useStore((s) => s.settings.url);
  const local = useStore((s) => s.local);
  const [asked, setAsked] = useState(false);

  useEffect(() => {
    void refreshLocal().then(() => setAsked(true));
  }, []);

  // In the desktop app, an empty machine opens on a blank certificate.
  useEffect(() => {
    if (hasKeeper && asked && local && local.ghosts.length === 0 && watch.is !== "open") open("birth");
  }, [asked, local, watch.is]);

  if (hasKeeper && local && local.ghosts.length > 0) return <Ghosts />;
  return (
    <article className="paper is-plain">
      <header className="paper-head">
        <span className="label">{watch.is === "connecting" ? "looking" : "nobody home"}</span>
        <h1>{watch.is === "refused" ? "The daemon said no" : "Nobody is awake"}</h1>
        <p className="paper-lede">
          {watch.is === "refused" ? watch.why : `No daemon answers at ${url}.`}{" "}
          {hasKeeper ? "" : "Start one, and this window will find it."}
        </p>
        {!hasKeeper ? <p className="mono-line">ghostd run ghosts/&lt;name&gt;</p> : null}
        <div className="acts">
          <button className="act is-quiet" onClick={() => open("body")}>
            Connection
          </button>
        </div>
      </header>
    </article>
  );
}
