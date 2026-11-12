// The way to the keeper: the part of the desktop shell that keeps ghosts on
// this machine (birth, models, waking, resting, pictures). In the desktop app
// it is one Tauri command. Under `vite dev` the same code is reached through
// a small bridge, so the window can be worked on in a browser. A plain web
// build has no keeper: it can talk to one daemon running somewhere else.

import { invoke } from "@tauri-apps/api/core";

const inShell = "__TAURI_INTERNALS__" in window;

export const hasKeeper = inShell || import.meta.env.DEV;

export async function keep<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (inShell) {
    try {
      return await invoke<T>("keep", { cmd, args });
    } catch (why) {
      throw new Error(String(why));
    }
  }
  if (import.meta.env.DEV) {
    const response = await fetch("/__keeper", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ cmd, args }),
    });
    const answer = (await response.json()) as { ok?: T; err?: string };
    if (answer.err !== undefined) throw new Error(answer.err);
    return answer.ok as T;
  }
  throw new Error("Keeping a ghost on this machine needs the desktop app.");
}

/** A ghost that lives on this machine. */
export type LocalGhost = {
  path: string;
  slug: string;
  name: string;
  pronouns: string;
  city: string;
  country: string;
  born: string;
  seal: string;
  /** The id of the model she thinks with. */
  model: string | null;
  /** Where her daemon listens, when it is running. */
  url: string | null;
  /** Her picture, as a data URL. */
  avatar: string | null;
};

export type ModelKind = "anthropic" | "openai" | "claude-code" | "codex";

/** A brain available on this machine. The key is write-only. */
export type Model = {
  id: string;
  kind: ModelKind;
  model: string;
  base_url: string | null;
  api_key?: string | null;
  api_key_env: string | null;
  json_mode: boolean;
};

export type Local = {
  home: string;
  ghosts: LocalGhost[];
  models: { has_key: boolean; model: Model }[];
  tempo: number;
  ghostd: string | null;
};

/** Who serves a model, in a word. */
export function provider(model: Model): string {
  if (model.kind === "anthropic") return "Anthropic";
  if (model.kind === "claude-code") return "Claude Code";
  if (model.kind === "codex") return "Codex";
  if (!model.base_url) return "OpenAI";
  try {
    const host = new URL(model.base_url).hostname;
    if (host === "localhost" || host === "127.0.0.1") return "Local";
    const parts = host.split(".");
    const name = parts.length > 1 ? parts[parts.length - 2]! : host;
    return name.charAt(0).toUpperCase() + name.slice(1);
  } catch {
    return "OpenAI-compatible";
  }
}

/** What to call a model. The CLI relays may have no model name of their own. */
export function label(model: Model): string {
  return model.model && model.model !== model.kind ? model.model : provider(model);
}

/** A steady colour for a model, so it is recognisable at a glance. */
export function tint(model: Model): string {
  if (model.kind === "anthropic" || model.kind === "claude-code") return "#d97757";
  let hash = 0;
  for (const c of model.base_url ?? model.kind) hash = (hash * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${hash % 360} 70% 62%)`;
}
