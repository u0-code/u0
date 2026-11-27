# u0-telemetry

The Cloudflare Worker `u0-telemetry` (https://u0-telemetry.aac6fef-aed.workers.dev,
`telemetry::ENDPOINT`) that takes the batches u0 sends
(`crates/core/src/telemetry.rs`; what each level sends is in the main
README). It checks each batch against `batch.schema.json`, refuses one
with a field it does not know or a value of the wrong shape, and keeps it
as a row of the D1 database `ghost-telemetry` (`schema.sql`; it keeps the
name it was made with before the product was u0): the batch
and the country Cloudflare saw it from, nothing else of the request.

`batch.schema.json` is made from `telemetry::Batch`, the Rust that sends
the batches: `cargo test -p u0-core telemetry` writes it anew, and
fails, when it was not the batch's. Deploy the Worker with the new one.

Deploy it with Cloudflare's `cf` CLI, logged in:

```sh
cd telemetry
npm ci
npx cf deploy        # binds the D1 database ghost-telemetry, made the first time
cf d1 list           # its id, for the commands below
cf d1 query <id> --sql="$(cat schema.sql)"
```

## Reading it

```sh
# The versions in use this week: installs by version and channel.
cf d1 query <id> --sql "SELECT version, channel, count(DISTINCT id) AS installs, count(*) AS batches FROM batches WHERE at > datetime('now', '-7 days') GROUP BY version, channel ORDER BY installs DESC"

# The agents used this week, by the installs that use each.
cf d1 query <id> --sql "SELECT a.key AS agent, count(DISTINCT b.id) AS installs FROM batches b, json_each(b.agents) a WHERE b.at > datetime('now', '-7 days') GROUP BY agent ORDER BY installs DESC"

# How far installs let u0 see: the level each sent last this week, and
# the first launches (level 0, sent once and with no id).
cf d1 query <id> --sql "SELECT level, count(*) AS installs FROM (SELECT id, max(level) AS level FROM batches WHERE level > 0 AND at > datetime('now', '-7 days') GROUP BY id) GROUP BY level UNION ALL SELECT 'first launches', count(*) FROM batches WHERE level = 0 AND at > datetime('now', '-7 days')"
```

A row is deleted as any is: `cf d1 query <id> --sql "DELETE FROM batches WHERE ..."`.
