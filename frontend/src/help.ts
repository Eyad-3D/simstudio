// The help pages (LRN-04) are static files the engine serves at /help/, built
// from the repo's Markdown by scripts/build-docs.mjs. They open in a new
// browser tab (the desktop app hands web links to the system browser), so
// they work with no network and keep the browser's tabs and bookmarks.

/** The help page of a library part, by its id ("signal.driving_task"). */
export const componentHelpPage = (defId: string) => `reference/components/${defId}.html`;

export function openHelp(page = "index.html"): void {
  window.open(`${import.meta.env.BASE_URL}help/${page}`, "_blank", "noopener");
}
