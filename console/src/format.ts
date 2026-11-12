// Small formatting helpers. Times are shown in her timezone, not the
// reader's: this is a window onto her day.

import type { PlaceRow, Reach, Stage } from "./protocol/types";

export function clock(at: string, timeZone?: string): string {
  return new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", timeZone }).format(new Date(at));
}

/** `Thursday, 12 November`. */
export function day(at: string, timeZone?: string): string {
  const parts = new Intl.DateTimeFormat("en-GB", { weekday: "long", day: "numeric", month: "long", timeZone }).formatToParts(
    new Date(at),
  );
  const get = (type: string) => parts.find((part) => part.type === type)?.value ?? "";
  return `${get("weekday")}, ${get("day")} ${get("month")}`;
}

/** `2026-11-12`, the calendar day in a timezone. */
export function dayKey(at: string, timeZone?: string): string {
  return new Intl.DateTimeFormat("en-CA", { timeZone }).format(new Date(at));
}

/** A day key as a date at noon UTC, safe to format in any timezone as that day. */
export function noonOf(key: string): string {
  return `${key}T12:00:00Z`;
}

/** How long ago, in a word or two. */
export function ago(at: string | null, now = Date.now()): string {
  if (!at) return "";
  const seconds = Math.max(0, (now - new Date(at).getTime()) / 1000);
  if (seconds < 90) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  const days = Math.floor(seconds / 86400);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

/** How long until, in words. */
export function until(at: string, now = Date.now()): string {
  const seconds = (new Date(at).getTime() - now) / 1000;
  if (seconds <= 5) return "any moment now";
  if (seconds < 90) return `in ${Math.round(seconds)} seconds`;
  if (seconds < 5400) return `in ${Math.round(seconds / 60)} minutes`;
  return `in ${Math.round(seconds / 3600)} hours`;
}

/** The length of a wait: `4s`, `1m 53s`, `2h 10m`. */
export function span(from: string, to: string): string {
  const seconds = Math.max(0, Math.round((new Date(to).getTime() - new Date(from).getTime()) / 1000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  return `${Math.floor(seconds / 86400)}d ${Math.floor((seconds % 86400) / 3600)}h`;
}

export function lastActive(row: PlaceRow): string | null {
  const { last_in_at, last_out_at } = row.place;
  if (!last_in_at) return last_out_at;
  if (!last_out_at) return last_in_at;
  return last_in_at > last_out_at ? last_in_at : last_out_at;
}

/** What someone is to her, as the end of "… is ___ to her". */
export const STANDING: Record<Stage, string> = {
  gone: "someone she has left",
  estranged: "someone she has pulled far away from",
  cooling: "someone she is cooling toward",
  stranger: "a stranger",
  acquaintance: "an acquaintance",
  friend: "a friend",
  close: "close",
};

/** What an activity leaves of her attention. */
export const REACH: Record<Reach, string> = {
  free: "her phone is in her hand",
  glance: "she glances at her phone now and then",
  busy: "she is occupied and rarely looks",
  away: "she is not looking at her phone",
};

export function capital(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
