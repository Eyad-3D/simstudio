import { useEffect, useRef, useState } from "react";
import { useUIStore } from "../store/uiStore";

/** Renders the styled confirm/prompt modal requested via dialog.ts helpers.
 *  Mounted once (in App). Resolves the caller's promise on confirm/cancel. */
export function DialogHost() {
  const dialog = useUIStore((s) => s.dialog);
  const closeDialog = useUIStore((s) => s.closeDialog);
  const [text, setText] = useState("");
  const inputRef = useRef<HTMLInputElement | null>(null);

  // seed the prompt input each time a dialog opens; focus + select it
  useEffect(() => {
    if (!dialog) return;
    setText(dialog.defaultValue ?? "");
    const t = setTimeout(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    }, 0);
    return () => clearTimeout(t);
  }, [dialog]);

  useEffect(() => {
    if (!dialog) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") settle(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dialog]);

  if (!dialog) return null;

  const settle = (value: boolean | string | null) => {
    dialog.resolve(value);
    closeDialog();
  };
  const onConfirm = () => settle(dialog.kind === "prompt" ? text : true);
  const onCancel = () => settle(dialog.kind === "prompt" ? null : false);

  return (
    <div
      className="fixed inset-0 z-[110] flex items-center justify-center bg-black/35"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <div className="w-[380px] max-w-[92vw] overflow-hidden rounded-md border border-[color:var(--ss-border)] bg-[color:var(--ss-panel)] shadow-2xl">
        <div className="border-b border-[color:var(--ss-border)] bg-[color:var(--ss-panel-alt)] px-4 py-2.5 text-[13px] font-semibold">
          {dialog.title}
        </div>
        <div className="px-4 py-3">
          {dialog.message && (
            <p className="text-[12px] leading-snug text-[color:var(--ss-text-dim)]">
              {dialog.message}
            </p>
          )}
          {dialog.kind === "prompt" && (
            <input
              ref={inputRef}
              className="ss-input mt-2 w-full"
              value={text}
              placeholder={dialog.placeholder}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  onConfirm();
                }
              }}
            />
          )}
        </div>
        <div className="flex justify-end gap-2 border-t border-[color:var(--ss-border)] px-4 py-2.5">
          {dialog.altLabel && (
            <button
              className="ss-toolbtn mr-auto border border-[color:var(--ss-border)] px-3"
              onClick={() => settle("alt")}
            >
              {dialog.altLabel}
            </button>
          )}
          <button
            className="ss-toolbtn border border-[color:var(--ss-border)] px-3"
            onClick={onCancel}
          >
            {dialog.cancelLabel ?? "Cancel"}
          </button>
          <button
            className={`rounded px-3 py-1 text-[12px] font-semibold text-white ${
              dialog.danger ? "bg-red-600 hover:bg-red-700" : "bg-[color:var(--ss-accent)] hover:brightness-110"
            } disabled:opacity-40`}
            disabled={dialog.kind === "prompt" && text.trim() === ""}
            onClick={onConfirm}
          >
            {dialog.confirmLabel ?? "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}
