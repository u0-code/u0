// Ghost's telemetry: each batch a Ghost sends (crates/core/src/telemetry.rs),
// checked against its schema (batch.schema.json, made from the Rust that
// sends it) and kept as a row of D1's `batches` (schema.sql). Nothing of
// the request is kept but the country Cloudflare reads it from: no
// address, no headers, no other place.

import { Validator } from "@cfworker/json-schema";
import schema from "./batch.schema.json" with { type: "json" };

/** The most a batch may weigh, in bytes. */
const MOST = 8192;

/** What a batch may be: one of a level, every field of it and nothing more. */
const batches = new Validator(schema, "2020-12", false);

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
    const body = await request.arrayBuffer();
    if (body.byteLength > MOST) {
      return new Response(null, { status: 413 });
    }
    let batch;
    try {
      batch = JSON.parse(new TextDecoder().decode(body));
    } catch {
      return new Response(null, { status: 400 });
    }
    if (!batches.validate(batch).valid) {
      return new Response(null, { status: 400 });
    }
    const country = /^[A-Z0-9]{2}$/.test(request.cf?.country ?? "") ? request.cf.country : "";
    // The names are the schema's, checked above: none comes from the request.
    const names = Object.keys(batch);
    const columns = [...names, "country"].map((n) => `"${n}"`).join(", ");
    const marks = [...names, "country"].map(() => "?").join(", ");
    await env.DB.prepare(`INSERT INTO batches (${columns}) VALUES (${marks})`)
      .bind(...names.map((n) => cell(batch[n])), country)
      .run();
    return new Response(null, { status: 204 });
  },
};
