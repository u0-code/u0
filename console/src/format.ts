// Small formatting helpers. Times are shown in her timezone, not the viewer's:
// this is a window onto her day.

import type { PlaceRow, Reach, Stage } from "./protocol/types";

export function clock(at: string, timeZone?: string): string {
  return new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", timeZone }).format(new Date(at));
}

export function day(at: string, timeZone?: string): string {
  return new Intl.DateTimeFormat("en-US", { weekday: "long", month: "long", day: "numeric", timeZone }).format(
    new Date(at),
  );
}

export function dayKey(at: string, timeZone?: string): string {
  return new Intl.DateTimeFormat("en-CA", { timeZone }).format(new Date(at));
}

/** `11-12 15:21:22`: month, day, then the time, as her journal prints it. */
export function stamp(at: string, timeZone?: string): string {
  const parts = new Intl.DateTimeFormat("en-GB", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    timeZone,
  }).formatToParts(new Date(at));
  const get = (type: string) => parts.find((part) => part.type === type)?.value ?? "";
  return `${get("month")}-${get("day")} ${get("hour")}:${get("minute")}:${get("second")}`;
}

/** How long ago, as short as a sidebar needs it. */
export function ago(at: string | null, now = Date.now()): string {
  if (!at) return "";
  const seconds = Math.max(0, (now - new Date(at).getTime()) / 1000);
  if (seconds < 60) return "now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  if (seconds < 86400 * 7) return `${Math.floor(seconds / 86400)}d`;
  return `${Math.floor(seconds / (86400 * 7))}w`;
}

/** How long until, in words. */
export function until(at: string, now = Date.now()): string {
  const seconds = (new Date(at).getTime() - now) / 1000;
  if (seconds <= 5) return "any moment";
  if (seconds < 90) return `in ${Math.round(seconds)}s`;
  if (seconds < 5400) return `in ${Math.round(seconds / 60)} min`;
  return `in ${Math.round(seconds / 3600)} h`;
}

export function placeTitle(row: PlaceRow): string {
  const { place } = row;
  if (place.kind === "direct") return row.with ?? place.id;
  if (place.name) return place.name;
  return place.kind === "public" ? `Thread ${place.id.slice(-6)}` : place.id;
}

export function lastActive(row: PlaceRow): string | null {
  const { last_in_at, last_out_at } = row.place;
  if (!last_in_at) return last_out_at;
  if (!last_out_at) return last_in_at;
  return last_in_at > last_out_at ? last_in_at : last_out_at;
}

export const STAGE_WORD: Record<Stage, string> = {
  gone: "left",
  estranged: "estranged",
  cooling: "cooling",
  stranger: "stranger",
  acquaintance: "acquaintance",
  friend: "friend",
  close: "close",
};

export function signed(n: number): string {
  return n > 0 ? `+${n}` : String(n);
}

/** The length of a wait, the way a person would say it: `4s`, `1m 53s`, `2h 10m`. */
export function span(from: string, to: string): string {
  const seconds = Math.max(0, Math.round((new Date(to).getTime() - new Date(from).getTime()) / 1000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  return `${Math.floor(seconds / 86400)}d ${Math.floor((seconds % 86400) / 3600)}h`;
}

/** How soon, as short as a sidebar needs it. */
export function shortly(at: string, now = Date.now()): string {
  const seconds = (new Date(at).getTime() - now) / 1000;
  if (seconds <= 5) return "now";
  if (seconds < 90) return `${Math.round(seconds)}s`;
  if (seconds < 5400) return `${Math.round(seconds / 60)} min`;
  return `${Math.round(seconds / 3600)} h`;
}

/** What an activity leaves of her attention. */
export const REACH_WORDS: Record<Reach, string> = {
  free: "her phone is in her hand",
  glance: "she glances at her phone now and then",
  busy: "she is occupied and rarely looks",
  away: "she is not looking at her phone",
};
