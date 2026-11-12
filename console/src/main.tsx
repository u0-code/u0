import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { start, useActive, useStore } from "./store";
import "./styles.css";
import { About, Certificate } from "./views/About";
import { Birth } from "./views/Birth";
import { Chat } from "./views/Chat";
import { Settings } from "./views/Settings";
import { Rail, Sidebar, TitleBar } from "./views/Shell";

// Inside the desktop shell the title bar shares its row with the window
// controls, so it leaves them room.
document.documentElement.dataset.shell = "__TAURI_INTERNALS__" in window ? "tauri" : "web";

function Main() {
  const session = useActive();
  const view = useStore((s) => s.view);
  const panel = useStore((s) => s.panel);
  const local = useStore((s) => s.local);

  if (view === "settings") return <Settings />;
  if (view === "birth" && local) return <Birth />;
  // A machine with nobody on it opens on a blank certificate: there is
  // nothing else to do here until someone exists.
  if (!session) return local ? <Birth /> : null;
  if (view === "certificate") return <Certificate session={session} />;
  return (
    <>
      <Chat session={session} key={session.id} />
      {panel && <About session={session} />}
    </>
  );
}

function App() {
  const sidebar = useStore((s) => s.sidebar);
  return (
    <div className="app">
      <TitleBar />
      <div className="body">
        {sidebar ? <Sidebar /> : <Rail />}
        <main className="main">
          <Main />
        </main>
      </div>
    </div>
  );
}

start();
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
