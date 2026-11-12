// A birth certificate as text, and the seal of that text.
//
// The window writes the exact bytes of `ghost.toml` and hashes them with
// BLAKE3, which is what the core does at birth. The keeper writes those bytes
// unchanged, so the mark that formed on screen while the certificate was
// being written is the seal she is born with.

import { blake3 } from "@noble/hashes/blake3.js";
import { bytesToHex, utf8ToBytes } from "@noble/hashes/utils.js";

export type Draft = {
  name: string;
  pronouns: string;
  born: string;
  city: string;
  country: string;
  timezone: string;
  wake: string;
  sleep: string;
  temperament: string;
  voice: string;
  origin: string;
  values: string;
  cares: string;
  cannot: string;
};

export function blank(): Draft {
  let timezone = "UTC";
  try {
    timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {
    // keep UTC
  }
  return {
    name: "",
    pronouns: "she/her",
    born: new Intl.DateTimeFormat("en-CA").format(new Date()),
    city: "",
    country: "",
    timezone,
    wake: "08:30",
    sleep: "00:30",
    temperament: "",
    voice: "",
    origin: "",
    values: "",
    cares: "",
    cannot: "",
  };
}

/** A one-line TOML string. JSON's escapes are a subset of TOML's. */
const line = (text: string) => JSON.stringify(text.trim());

/** A multi-line TOML string, kept readable for whoever opens the file. */
function prose(text: string): string {
  const body = text.trim().replace(/\\/g, "\\\\").replace(/"""/g, '""\\"');
  return `"""\n${body}\n"""`;
}

function list(text: string): string {
  const items = text
    .split(/[,\n，、]/)
    .map((item) => item.trim())
    .filter(Boolean);
  return `[${items.map((item) => JSON.stringify(item)).join(", ")}]`;
}

/** The text of `ghost.toml` for a draft. Deterministic: same draft, same bytes. */
export function certificate(d: Draft): string {
  return `# The birth certificate of ${d.name.trim() || "a ghost"}.
# Written once, then sealed. After birth nothing here can change.

name = ${line(d.name)}
pronouns = ${line(d.pronouns)}
born = ${line(d.born)}

temperament = ${prose(d.temperament)}

voice = ${prose(d.voice)}

origin = ${prose(d.origin)}

values = ${list(d.values)}
cares_about = ${list(d.cares)}
cannot_stand = ${list(d.cannot)}

[home]
city = ${line(d.city)}
country = ${line(d.country)}
timezone = ${line(d.timezone)}

[rhythm]
wake = ${line(d.wake)}
sleep = ${line(d.sleep)}
`;
}

/** The BLAKE3 of the certificate's bytes, as hex: her seal. */
export function seal(text: string): string {
  return bytesToHex(blake3(utf8ToBytes(text)));
}

/** What still has to be written before she can be born, if anything. */
export function missing(d: Draft): string | null {
  if (!d.name.trim()) return "She needs a name.";
  if (!d.city.trim()) return "She needs somewhere to live.";
  try {
    new Intl.DateTimeFormat("en", { timeZone: d.timezone.trim() });
  } catch {
    return `"${d.timezone}" is not a timezone. Try something like Asia/Shanghai.`;
  }
  const time = /^([01]\d|2[0-3]):[0-5]\d$/;
  if (!time.test(d.wake.trim()) || !time.test(d.sleep.trim())) return "Waking and sleeping times look like 08:30.";
  if (!/^\d{4}-\d{2}-\d{2}$/.test(d.born.trim())) return "Her birthday looks like 2026-11-12.";
  if (d.temperament.trim().length < 20) return "Say more about who she is.";
  if (d.voice.trim().length < 10) return "Say how she talks.";
  if (d.origin.trim().length < 10) return "Say where she came from.";
  return null;
}
