import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

const here = dirname(fileURLToPath(import.meta.url));

/**
 * In the desktop app the window reaches the keeper through a Tauri command.
 * Under `vite dev` there is no shell, so this forwards the same requests to
 * the shell's own binary (`ghost-console keeper`). It lets the window be
 * worked on in a browser against the code the app runs. Development only:
 * it is not part of any build, and it refuses requests from other sites.
 */
function keeperBridge(): Plugin {
  return {
    name: "ghost-keeper-bridge",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use("/__keeper", (req, res) => {
        const answer = (body: object, status = 200) => {
          res.statusCode = status;
          res.setHeader("content-type", "application/json");
          res.end(JSON.stringify(body));
        };
        const origin = req.headers.origin;
        const sameSite = origin !== undefined && new URL(origin).host === req.headers.host;
        if (req.method !== "POST" || !sameSite) return answer({ err: "not from here" }, 403);

        const binary = resolve(here, "src-tauri/target/debug/ghost-console");
        if (!existsSync(binary)) {
          return answer({ err: "build the shell first: cargo build, in console/src-tauri" });
        }
        let request = "";
        req.on("data", (chunk) => (request += chunk));
        req.on("end", () => {
          // In a source checkout, ghosts live in the checkout.
          const env = { ...process.env, GHOST_HOME: process.env.GHOST_HOME ?? resolve(here, "..") };
          const keeper = spawn(binary, ["keeper"], { env });
          let said = "";
          keeper.stdout.on("data", (chunk) => (said += chunk));
          keeper.on("error", (error) => answer({ err: String(error) }));
          keeper.on("close", () => {
            res.setHeader("content-type", "application/json");
            res.end(said.trim() || JSON.stringify({ err: "the keeper said nothing" }));
          });
          keeper.stdin.end(request);
        });
      });
    },
  };
}

// The port is fixed because the Tauri shell points at it in development.
export default defineConfig({
  plugins: [react(), keeperBridge()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", outDir: "dist" },
});
