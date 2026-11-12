import "@fontsource-variable/newsreader/opsz.css";
import "@fontsource-variable/newsreader/opsz-italic.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { connect, open, useStore } from "./store";
import "./styles.css";
import { Birth, Certificate } from "./views/Birth";
import { Body, Ghosts, Nobody } from "./views/Body";
import { Now } from "./views/Now";
import { Record } from "./views/Record";

// Inside the desktop shell the top strip shares its row with the window
// controls, so it needs to leave them room.
document.documentElement.dataset.shell = "__TAURI_INTERNALS__" in window ? "tauri" : "web";

// Dawn and dusk fade slowly, but opening the window should not replay one.
document.documentElement.classList.add("is-opening");
setTimeout(() => document.documentElement.classList.remove("is-opening"), 2500);

/** The window is lit by her day: paper while she is awake, night while she
 *  sleeps. Whoever sits here can ask for their own computer's light instead. */
function useLight() {
  const phase = useStore((s) => s.status?.moment.phase);
  const light = useStore((s) => s.settings.light);
  useEffect(() => {
    const system = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const night = light === "hers" && phase ? phase === "asleep" : system.matches;
      document.documentElement.dataset.light = night ? "night" : "day";
    };
    apply();
    system.addEventListener("change", apply);
    return () => system.removeEventListener("change", apply);
  }, [phase, light]);
}

function App() {
  const sheet = useStore((s) => s.sheet);
  const connected = useStore((s) => s.watch.is === "open" && s.ghost !== null);
  // On an empty machine the blank certificate is the first and only page.
  const somewhere = useStore((s) => connected || (s.local?.ghosts.length ?? 0) > 0);
  useLight();

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") open(null);
    };
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, []);

  return (
    <div className="app">
      <div className="strip" data-tauri-drag-region>
        {sheet && (somewhere || sheet !== "birth") ? (
          <button className="label link" onClick={() => open(null)}>
            ‹ back
          </button>
        ) : null}
      </div>
      {sheet ? (
        <div className="desk" key={sheet}>
          {sheet === "birth" ? <Birth /> : sheet === "certificate" ? <Certificate /> : sheet === "body" ? <Body /> : <Ghosts />}
        </div>
      ) : connected ? (
        <div className="stage">
          <Now />
          <Record />
        </div>
      ) : (
        <div className="desk">
          <Nobody />
        </div>
      )}
    </div>
  );
}

connect();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
