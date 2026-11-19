// Help search (LRN-04): a page matches when every word typed is in its title
// or text; title matches first. The index is search-index.js (HELP_INDEX).
const q = document.getElementById("q");
const hits = document.getElementById("hits");
const up = document.querySelector('link[rel="stylesheet"]').getAttribute("href").replace("help.css", "");
q.addEventListener("input", () => {
  const words = q.value.toLowerCase().split(/\s+/).filter(Boolean);
  const inTitle = (p) => words.every((w) => p.t.toLowerCase().includes(w));
  const found = words.length
    ? window.HELP_INDEX.filter((p) => words.every((w) => (p.t + " " + p.x).toLowerCase().includes(w)))
        .sort((a, b) => inTitle(b) - inTitle(a))
        .slice(0, 15)
    : [];
  hits.replaceChildren(
    ...found.map((p) => {
      const li = document.createElement("li");
      const a = document.createElement("a");
      a.href = up + p.u;
      a.textContent = `${p.t} (${p.s})`;
      li.append(a);
      return li;
    }),
  );
  if (words.length && !found.length) {
    const li = document.createElement("li");
    li.textContent = "No page matches.";
    hits.append(li);
  }
});
