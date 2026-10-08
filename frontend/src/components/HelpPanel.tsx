// The Help panel (LRN-09): the help pages inside the app, beside the work,
// so they open with no network and without leaving the window. It shows the
// page the help store names (help.ts); F1, the "?" buttons and the Help
// menu set it. The pages are the engine's own (/help/), same origin, so the
// panel can follow their links: a link out of the help (GitHub) goes to the
// system browser, as the panel's "Open in browser" does for the page itself.
import { useRef, useState } from "react";
import { ArrowLeft, ExternalLink, House, X } from "lucide-react";
import { closeHelp, helpUrl, openHelpInBrowser, useHelpStore } from "../help";
import { HELP_WIDTH_KEY } from "../storageKeys";

const MIN_WIDTH = 320;
const MAX_SHARE = 0.6; // of the window

function loadWidth(): number {
  try {
    const saved = Number(window.localStorage.getItem(HELP_WIDTH_KEY));
    if (Number.isFinite(saved) && saved >= MIN_WIDTH) return saved;
  } catch {
    /* storage unavailable */
  }
  return 440;
}

/** The help page an iframe shows, as a path under /help/ ("glossary.html#soc"). */
function shownPage(frame: HTMLIFrameElement | null): string | null {
  try {
    const loc = frame?.contentWindow?.location;
    const base = new URL(helpUrl(""), window.location.href).pathname;
    return loc && loc.pathname.startsWith(base) ? loc.pathname.slice(base.length) + loc.hash : null;
  } catch {
    return null; // not the engine's page
  }
}

export function HelpPanel() {
  const page = useHelpStore((s) => s.page);
  const frame = useRef<HTMLIFrameElement>(null);
  const [width, setWidth] = useState(loadWidth);
  const [title, setTitle] = useState("Help");

  if (!page) return null;

  const onLoad = () => {
    const doc = frame.current?.contentDocument;
    if (!doc) return;
    setTitle(doc.title.replace(/ · LightSim .*$/, "") || "Help");
    // links out of the help open in the system browser: other sites refuse
    // to show inside a frame, and the panel is for the help
    doc.addEventListener("click", (e) => {
      const a = (e.target as Element | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
      if (!a || new URL(a.href).origin === window.location.origin) return;
      e.preventDefault();
      window.open(a.href, "_blank", "noopener");
    });
    doc.addEventListener("keydown", (e) => {
      if (e.key === "Escape") closeHelp();
      if (e.key === "F1") e.preventDefault();
    });
  };

  const startResize = (e: React.PointerEvent) => {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = width;
    // the frame would swallow the pointer while it passes over it
    if (frame.current) frame.current.style.pointerEvents = "none";
    const move = (ev: PointerEvent) =>
      setWidth(Math.round(Math.min(window.innerWidth * MAX_SHARE, Math.max(MIN_WIDTH, w0 + x0 - ev.clientX))));
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (frame.current) frame.current.style.pointerEvents = "";
      setWidth((w) => {
        try {
          window.localStorage.setItem(HELP_WIDTH_KEY, String(w));
        } catch {
          /* storage unavailable */
        }
        return w;
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  return (
    <aside
      className="ss-zoom relative flex min-h-0 shrink-0 flex-col border-l border-[color:var(--ss-border)] bg-[color:var(--ss-panel)]"
      style={{ width }}
      aria-label="Help"
      onKeyDown={(e) => e.key === "Escape" && closeHelp()}
    >
      <div
        className="absolute inset-y-0 -left-1 w-2 cursor-col-resize"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize the help"
        onPointerDown={startResize}
      />
      <div className="ss-panel-toolbar flex items-center gap-1 border-b border-[color:var(--ss-border)] px-1 py-0.5">
        <button
          className="ss-toolbtn"
          title="Back"
          aria-label="Back"
          onClick={() => frame.current?.contentWindow?.history.back()}
        >
          <ArrowLeft size={13} />
        </button>
        <button
          className="ss-toolbtn"
          title="Help front page"
          aria-label="Help front page"
          onClick={() => useHelpStore.setState({ page: "index.html" })}
        >
          <House size={13} />
        </button>
        <h2 className="min-w-0 flex-1 truncate px-1 text-[12px] font-semibold" title={title}>
          {title}
        </h2>
        <button
          className="ss-toolbtn"
          title="Open in your web browser, for tabs and bookmarks"
          aria-label="Open in browser"
          onClick={() => openHelpInBrowser(shownPage(frame.current) ?? page)}
        >
          <ExternalLink size={13} />
        </button>
        <button className="ss-toolbtn" title="Close the help (Esc)" aria-label="Close the help" onClick={closeHelp}>
          <X size={13} />
        </button>
      </div>
      <iframe
        ref={frame}
        key={page}
        title="Help page"
        src={helpUrl(page)}
        className="min-h-0 w-full flex-1 border-0 bg-white"
        onLoad={onLoad}
      />
    </aside>
  );
}
