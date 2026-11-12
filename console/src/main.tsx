import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { connect, useStore } from "./store";
import "./styles.css";
import { SideBar, TitleBar } from "./views/Chrome";
import { JournalPage, LifePage, PeoplePage } from "./views/Pages";
import { Inspector, Thread } from "./views/Thread";

// Inside the desktop shell the title bar shares its row with the window
// controls, so it needs to leave them room.
const inShell = "__TAURI_INTERNALS__" in window;
document.documentElement.dataset.shell = inShell ? "tauri" : "web";

function Unreachable() {
  const watch = useStore((s) => s.watch);
  const url = useStore((s) => s.settings.url);
  if (watch.is === "open") return null;
  return (
    <div className="unreachable">
      <div className="unreachable-title">
        {watch.is === "connecting" ? "Looking for her" : watch.is === "refused" ? "The daemon said no" : "Nobody home"}
      </div>
      <div className="unreachable-line">
        {watch.is === "connecting" ? url : watch.why}
      </div>
      {watch.is === "closed" ? (
        <pre className="unreachable-hint">ghostd run ghosts/&lt;name&gt;</pre>
      ) : null}
    </div>
  );
}

function App() {
  const view = useStore((s) => s.view);
  const sidebar = useStore((s) => s.sidebar);
  const inspector = useStore((s) => s.inspector);
  const connected = useStore((s) => s.watch.is === "open" || s.ghost !== null);
  return (
    <div className="app">
      <TitleBar />
      <div className="frame">
        {sidebar ? <SideBar /> : null}
        <main className="main">
          {!connected ? (
            <Unreachable />
          ) : view.is === "thread" ? (
            <Thread />
          ) : view.is === "journal" ? (
            <JournalPage />
          ) : view.is === "people" ? (
            <PeoplePage />
          ) : (
            <LifePage />
          )}
        </main>
        {connected && inspector && view.is === "thread" ? <Inspector /> : null}
      </div>
    </div>
  );
}

connect();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
