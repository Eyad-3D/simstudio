import { useEffect, useRef } from "react";
import { AlertCircle, AlertTriangle, Ban, Info } from "lucide-react";
import { useProjectStore } from "../../store/projectStore";

/** The level's icon, named for screen readers. */
export function levelIcon(level: "info" | "warning" | "error", size = 13) {
  switch (level) {
    case "error":
      return <AlertCircle size={size} className="shrink-0 text-[color:var(--ss-err)]" role="img" aria-label="Error" />;
    case "warning":
      return <AlertTriangle size={size} className="shrink-0 text-[color:var(--ss-warn)]" role="img" aria-label="Warning" />;
    default:
      return <Info size={size} className="shrink-0 text-[color:var(--ss-accent)]" role="img" aria-label="Info" />;
  }
}

export function MessagesPanel() {
  const messages = useProjectStore((s) => s.messages);
  const clear = useProjectStore((s) => s.clearMessages);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "instant", block: "end" });
  }, [messages.length]);

  return (
    <div className="flex h-full flex-col">
      <div className="ss-panel-toolbar">
        <span className="text-[11px] text-[color:var(--ss-text-dim)]">
          {messages.length} message(s)
        </span>
        <button className="ss-toolbtn ml-auto" onClick={clear} title="Clear messages">
          <Ban size={12} /> Clear
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto font-mono text-[11.5px]">
        {messages.map((m, i) => (
          <div
            key={i}
            className={`flex items-start gap-2 border-b border-[color:var(--ss-td-border)] px-2 py-[3px] ${
              m.level === "error"
                ? "bg-red-50 dark:bg-red-950/40"
                : m.level === "warning"
                  ? "bg-amber-50 dark:bg-amber-950/40"
                  : ""
            }`}
          >
            <span className="text-[color:var(--ss-text-dim)]">{m.time}</span>
            <span className="mt-[1px]">{levelIcon(m.level, 12)}</span>
            <span className="whitespace-pre-wrap">{m.text}</span>
          </div>
        ))}
        {messages.length === 0 && (
          <div className="px-3 py-2 text-[12px] text-[color:var(--ss-text-dim)]">
            No messages yet.
          </div>
        )}
        <div ref={endRef} />
      </div>
    </div>
  );
}
