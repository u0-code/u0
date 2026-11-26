// The Worker that takes Ghost's telemetry (README.md): each batch a row of
// a D1 database, and a naive limit on how often one place may send.
import { bindings, defineConfig } from "cf/config";

export default defineConfig({
	worker: {
		name: "ghost-telemetry",
		compatibilityDate: "2026-11-01",
		entrypoint: "worker.js",
		workersDev: true,
		env: {
			DB: bindings.d1({ name: "ghost-telemetry" }),
			LIMIT: bindings.rateLimit({
				namespace: "4317",
				simple: { limit: 20, period: 60 },
			}),
		},
	},
});
