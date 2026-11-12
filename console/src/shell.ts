// The way to the keeper: the part of the desktop shell that keeps ghosts on
// this machine (birth, body, waking, resting). In the desktop app it is one
// Tauri command. Under `vite dev` the same code is reached through a small
// bridge, so the window can be worked on in a browser. A plain web build has
// no keeper: it can watch and talk, nothing more.

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
  name: string;
  pronouns: string;
  city: string;
  country: string;
  born: string;
  seal: string;
};

/** What a body runs on. The key is write-only: it never comes back. */
export type Body = {
  kind: "anthropic" | "openai" | "claude-code" | "codex";
  model: string;
  base_url: string | null;
  api_key?: string | null;
  api_key_env: string | null;
  json_mode: boolean;
  tempo: number;
};

export type Local = {
  home: string;
  ghosts: LocalGhost[];
  body: { configured: boolean; by_hand?: boolean; has_key: boolean; body: Body | null };
  running: { pid: number; path: string } | null;
  ghostd: string | null;
};
