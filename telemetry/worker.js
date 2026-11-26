// Ghost's telemetry: each batch a Ghost sends (crates/core/src/telemetry.rs),
// checked field by field and kept as a row of D1's `batches`
// (schema.sql). Nothing of the request is kept but the country Cloudflare
// reads it from: no address, no headers, no other place.

/** The most a batch may weigh, in bytes. */
const MOST = 8192;

/** The most entries a map may have. */
const ENTRIES = 64;

/** A name: an agent's id, a version, a tool, a page, a model, a kind. */
const NAME = /^[A-Za-z0-9][A-Za-z0-9._:@/+-]{0,63}$/;

const count = (v) => Number.isInteger(v) && v >= 0 && v <= 1e7;
const of = (...words) => (v) => words.includes(v);
const like = (re) => (v) => typeof v === "string" && re.test(v);
const map = (value) => (v) =>
  v !== null &&
  typeof v === "object" &&
  !Array.isArray(v) &&
  Object.keys(v).length <= ENTRIES &&
  Object.entries(v).every(([k, x]) => NAME.test(k) && value(x));

/** Each field, the level it is sent from, and what it may hold. A
 * field's name is its column's. */
const FIELDS = {
  level: [0, of(0, 1, 2, 3)],
  version: [0, like(/^\d{1,4}\.\d{1,4}\.\d{1,4}[0-9A-Za-z.+-]{0,24}$/)],
  commit: [0, like(/^[0-9a-f]{7,40}$/)],
  channel: [0, of("stable", "nightly", "dev")],
  os: [0, like(/^(macos \d{1,3}|linux)$/)],
  arch: [0, of("aarch64", "x86_64")],
  id: [1, like(/^[0-9a-f]{32}$/)],
  agents: [1, map(like(NAME))],
  servers: [1, count],
  listening: [1, of(true, false)],
  sessions: [2, map(count)],
  turns: [2, count],
  worktrees: [2, count],
  tasks: [2, count],
  runs: [2, count],
  tools: [2, map(count)],
  opened: [2, map(count)],
  crashes: [2, count],
  startup_ms: [3, count],
  memory_mb: [3, count],
  errors: [3, map(count)],
  models: [3, map(count)],
  width: [3, count],
  height: [3, count],
  zoom: [3, count],
  look: [3, of("light", "dark", "system")],
  language: [3, like(/^[a-z]{2,3}(-[A-Za-z0-9]{2,8})?$/)],
};

/** Whether `b` is a batch: every field of its level, and nothing more. */
function valid(b) {
  if (b === null || typeof b !== "object" || Array.isArray(b) || !of(0, 1, 2, 3)(b.level)) {
    return false;
  }
  const due = Object.keys(FIELDS).filter((k) => FIELDS[k][0] <= b.level);
  return Object.keys(b).length === due.length && due.every((k) => k in b && FIELDS[k][1](b[k]));
}

/** A field's value as its column holds it: a map as JSON, a switch as 0 or 1. */
const cell = (v) => (typeof v === "object" ? JSON.stringify(v) : typeof v === "boolean" ? Number(v) : v);

export default {
  async fetch(request, env) {
    if (request.method !== "POST") {
      return new Response(null, { status: 405 });
    }
    // Limited by where it comes from, which is not kept.
    const from = request.headers.get("cf-connecting-ip") ?? "";
    if (!(await env.LIMIT.limit({ key: from })).success) {
      return new Response(null, { status: 429 });
    }
    if (Number(request.headers.get("content-length")) > MOST) {
      return new Response(null, { status: 413 });
    }
    const body = await request.text();
    if (body.length > MOST) {
      return new Response(null, { status: 413 });
    }
    let batch;
    try {
      batch = JSON.parse(body);
    } catch {
      return new Response(null, { status: 400 });
    }
    if (!valid(batch)) {
      return new Response(null, { status: 400 });
    }
    const country = like(/^[A-Z0-9]{2}$/)(request.cf?.country) ? request.cf.country : "";
    // The names are FIELDS', checked above: none comes from the request.
    const names = Object.keys(batch);
    const columns = [...names, "country"].map((n) => `"${n}"`).join(", ");
    const marks = [...names, "country"].map(() => "?").join(", ");
    await env.DB.prepare(`INSERT INTO batches (${columns}) VALUES (${marks})`)
      .bind(...names.map((n) => cell(batch[n])), country)
      .run();
    return new Response(null, { status: 204 });
  },
};
