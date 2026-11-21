// The help pages take the app's light or dark theme and follow a change of
// it. The app saves its theme in the browser's storage, which the help
// shares (same origin: the Help window, the Help panel). With none saved (the
// help opened in another browser) the pages follow the system's setting.
// Loaded in <head>, before the page is drawn, so it never flashes the other
// theme.
(() => {
  const KEY = "lightsim-theme"; // frontend/src/storageKeys.ts THEME_KEY
  const apply = () => {
    let theme = null;
    try {
      theme = window.localStorage.getItem(KEY);
    } catch {
      /* storage unavailable: the system's setting */
    }
    if (theme === "dark" || theme === "light") document.documentElement.dataset.theme = theme;
    else delete document.documentElement.dataset.theme;
  };
  apply();
  // another window of the app (the main window) changed it
  window.addEventListener("storage", (e) => {
    if (e.key === KEY || e.key === null) apply();
  });
})();
