// What belongs to this machine and to whoever sits at it, not to any ghost:
// the models a ghost can think with, the name they know you by, and how fast
// time runs for them here.

import { useState } from "react";
import { Icon } from "../Icon";
import { label, provider, tint, type Model, type ModelKind } from "../shell";
import { deleteModel, saveModel, saveSettings, setTempo, useStore } from "../store";

type Preset = { label: string; kind: ModelKind; model: string; base_url: string; json_mode: boolean; env: string };

const PRESETS: Preset[] = [
  { label: "Anthropic", kind: "anthropic", model: "claude-sonnet-5-5", base_url: "", json_mode: false, env: "ANTHROPIC_API_KEY" },
  { label: "OpenAI", kind: "openai", model: "", base_url: "", json_mode: true, env: "OPENAI_API_KEY" },
  { label: "DeepSeek", kind: "openai", model: "deepseek-chat", base_url: "https://api.deepseek.com/v1", json_mode: true, env: "DEEPSEEK_API_KEY" },
  { label: "Ollama", kind: "openai", model: "", base_url: "http://localhost:11434/v1", json_mode: false, env: "" },
  { label: "Claude Code", kind: "claude-code", model: "", base_url: "", json_mode: false, env: "" },
  { label: "Codex", kind: "codex", model: "", base_url: "", json_mode: false, env: "" },
];

type Form = { id: string; label: string; kind: ModelKind; model: string; base_url: string; key: string; env: string; fromEnv: boolean; json_mode: boolean };

const fresh = (preset: Preset): Form => ({
  id: "",
  label: preset.label,
  kind: preset.kind,
  model: preset.model,
  base_url: preset.base_url,
  key: "",
  env: preset.env,
  fromEnv: false,
  json_mode: preset.json_mode,
});

const PACES = [
  { tempo: 1, label: "Human", note: "Minutes to notice a message, a night's sleep. The real thing." },
  { tempo: 0.15, label: "Quick", note: "The same life, about seven times faster. Good for getting to know someone new." },
  { tempo: 0, label: "Instant", note: "Answers at once, even asleep. For development only." },
];

/** Where a model's key comes from, as the end of its line in the list. */
function keyNote(model: Model, hasKey: boolean): string {
  if (model.kind === "claude-code" || model.kind === "codex") return " · your own sign-in";
  if (model.api_key_env) return ` · key from $${model.api_key_env}`;
  if (hasKey) return " · key saved";
  return model.base_url?.includes("localhost") ? "" : " · no key";
}

export function Settings() {
  const local = useStore((s) => s.local);
  const settings = useStore((s) => s.settings);
  const trouble = useStore((s) => s.trouble);
  const [form, setForm] = useState<Form | null>(null);
  // Removing a model throws its key away, so it takes a second press.
  const [removing, setRemoving] = useState<string | null>(null);
  const [you, setYou] = useState(settings.you);
  const [remote, setRemote] = useState(settings.remote);
  const [token, setToken] = useState(settings.token);

  const dirty = you !== settings.you || remote !== settings.remote || token !== settings.token;
  const cli = form?.kind === "claude-code" || form?.kind === "codex";

  const edit = (model: Model) =>
    setForm({
      id: model.id,
      label: provider(model),
      kind: model.kind,
      model: model.model === model.kind ? "" : model.model,
      base_url: model.base_url ?? "",
      key: "",
      env: model.api_key_env ?? "",
      fromEnv: model.api_key_env !== null,
      json_mode: model.json_mode,
    });

  const save = async () => {
    if (!form) return;
    const ok = await saveModel({
      id: form.id,
      kind: form.kind,
      model: form.model,
      base_url: form.base_url.trim() || null,
      api_key: form.fromEnv ? null : form.key || null,
      api_key_env: form.fromEnv ? form.env.trim() || null : null,
      json_mode: form.json_mode,
    });
    if (ok) setForm(null);
  };

  return (
    <div className="page">
      <div className="sheet-col">
        <h1>Settings</h1>

        {local && (
          <section className="set">
            <h3>Models</h3>
            <p>
              What a ghost thinks with. Any of them can be given to any ghost from the box you write in. Keys stay in a
              private file on this machine and are never shown again.
            </p>
            <div className="list">
              {local.models.map(({ model, has_key }) => (
                <div className="li" key={model.id}>
                  <i className="swatch" style={{ background: tint(model) }} />
                  <button className="li-t" onClick={() => edit(model)}>
                    <b>{label(model)}</b>
                    <span>
                      {provider(model)}
                      {keyNote(model, has_key)}
                    </span>
                  </button>
                  {removing === model.id ? (
                    <button className="btn danger" autoFocus onBlur={() => setRemoving(null)} onClick={() => void deleteModel(model.id)}>
                      Remove
                    </button>
                  ) : (
                    <button className="ib" onClick={() => setRemoving(model.id)} aria-label={`Remove ${label(model)}`}>
                      <Icon name="trash" small />
                    </button>
                  )}
                </div>
              ))}
              {local.models.length === 0 && <div className="li none">No models yet. Pick one below to add it.</div>}
            </div>

            {form === null ? (
              <div className="presets">
                {PRESETS.map((preset) => (
                  <button className="btn" key={preset.label} onClick={() => setForm(fresh(preset))}>
                    <Icon name="plus" small />
                    {preset.label}
                  </button>
                ))}
              </div>
            ) : (
              <div className="card">
                <div className="card-h">
                  <b>{form.id ? `Edit ${form.model || form.label}` : `Add ${form.label}`}</b>
                  <button className="ib" onClick={() => setForm(null)} aria-label="Close">
                    <Icon name="close" small />
                  </button>
                </div>
                {cli ? (
                  <p>
                    Uses the <code>{form.kind === "codex" ? "codex" : "claude"}</code> command line tool you are already
                    signed in to. Ghost runs it as a subprocess and never reads its credentials.
                  </p>
                ) : null}
                <div className="fields">
                  <label className="field wide">
                    <span>
                      Model
                      {cli && <small>Optional. Blank uses the tool's default.</small>}
                    </span>
                    <input
                      value={form.model}
                      onChange={(e) => setForm({ ...form, model: e.target.value })}
                      placeholder={cli ? "default" : "model name"}
                      spellCheck={false}
                      autoFocus
                    />
                  </label>
                  {!cli && (
                    <>
                      <label className="field wide">
                        <span>
                          Base URL
                          <small>Blank for the provider's own address.</small>
                        </span>
                        <input
                          value={form.base_url}
                          onChange={(e) => setForm({ ...form, base_url: e.target.value })}
                          placeholder={form.kind === "anthropic" ? "https://api.anthropic.com" : "https://api.openai.com/v1"}
                          spellCheck={false}
                        />
                      </label>
                      <label className="field wide">
                        <span>
                          {form.fromEnv ? "Environment variable holding the key" : "API key"}
                          <button className="link" type="button" onClick={() => setForm({ ...form, fromEnv: !form.fromEnv })}>
                            {form.fromEnv ? "Type the key instead" : "Use an environment variable"}
                          </button>
                        </span>
                        {form.fromEnv ? (
                          <input value={form.env} onChange={(e) => setForm({ ...form, env: e.target.value })} placeholder="DEEPSEEK_API_KEY" spellCheck={false} />
                        ) : (
                          <input
                            type="password"
                            value={form.key}
                            onChange={(e) => setForm({ ...form, key: e.target.value })}
                            placeholder={form.id ? "unchanged" : "sk-…"}
                            autoComplete="off"
                          />
                        )}
                      </label>
                      {form.kind === "openai" && (
                        <label className="check wide">
                          <input type="checkbox" checked={form.json_mode} onChange={(e) => setForm({ ...form, json_mode: e.target.checked })} />
                          <span>
                            Ask for JSON responses
                            <small>Turn off for servers that reject the option.</small>
                          </span>
                        </label>
                      )}
                    </>
                  )}
                </div>
                {trouble && <p className="trouble">{trouble}</p>}
                <div className="card-f">
                  <button className="btn" onClick={() => setForm(null)}>
                    Cancel
                  </button>
                  <button className="btn primary" onClick={() => void save()} disabled={!cli && !form.model.trim()}>
                    {form.id ? "Save" : "Add model"}
                  </button>
                </div>
              </div>
            )}
          </section>
        )}

        <section className="set">
          <h3>You</h3>
          <p>
            The name ghosts know you by here. Change it and you are someone they have not met: each conversation starts
            over, and the old one stays in their memory under the old name.
          </p>
          <div className="fields">
            <label className="field">
              <span>Your name</span>
              <input value={you} onChange={(e) => setYou(e.target.value)} spellCheck={false} />
            </label>
            {!local && (
              <>
                <label className="field wide">
                  <span>
                    Daemon address
                    <small>Where the ghost you are watching is running.</small>
                  </span>
                  <input value={remote} onChange={(e) => setRemote(e.target.value)} placeholder="ws://127.0.0.1:7437" spellCheck={false} />
                </label>
                <label className="field wide">
                  <span>Token</span>
                  <input type="password" value={token} onChange={(e) => setToken(e.target.value)} autoComplete="off" />
                </label>
              </>
            )}
          </div>
          {dirty && (
            <div className="card-f">
              <button
                className="btn"
                onClick={() => {
                  setYou(settings.you);
                  setRemote(settings.remote);
                  setToken(settings.token);
                }}
              >
                Revert
              </button>
              <button className="btn primary" onClick={() => saveSettings({ you: you.trim() || "you", remote: remote.trim(), token })}>
                Save
              </button>
            </div>
          )}
        </section>

        {local && (
          <section className="set">
            <h3>Pace</h3>
            <p>How fast time runs for the ghosts on this machine. It takes effect the next time each one is started.</p>
            <div className="seg" role="radiogroup">
              {PACES.map((pace) => {
                const on = Math.abs(local.tempo - pace.tempo) < 0.01;
                return (
                  <button key={pace.label} role="radio" aria-checked={on} className={on ? "on" : ""} onClick={() => void setTempo(pace.tempo)}>
                    {pace.label}
                  </button>
                );
              })}
            </div>
            <p className="quiet">
              {PACES.find((pace) => Math.abs(local.tempo - pace.tempo) < 0.01)?.note ?? `A custom pace of ${local.tempo}.`}
            </p>
          </section>
        )}

        {local && (
          <section className="set">
            <h3>This machine</h3>
            <dl className="kvs">
              <dt>Ghosts are kept in</dt>
              <dd>
                <code>{local.home}</code>
              </dd>
              <dt>Daemon</dt>
              <dd>{local.ghostd ? <code>{local.ghostd}</code> : <span className="trouble">ghostd was not found</span>}</dd>
            </dl>
            <p className="quiet">A ghost keeps running after this window is closed. Stop one from the menu at the top of her conversation.</p>
          </section>
        )}
      </div>
    </div>
  );
}
