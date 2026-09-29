// Build the help pages (LRN-04): the repo's Markdown and the component
// catalogue, rendered to static HTML in frontend/public/help (not tracked).
// Vite copies that folder into dist, the engine serves it at /help/ and the
// desktop app ships it, so the help opens with no network. Runs before
// `npm run dev` and `npm run build`, after sync-data.mjs.
//
// Pages come from docs/help/ (written by hand), README sections, the docs/
// pages, the examples, components.json and cycles.json. A link to a page or
// heading that does not exist, or a README section that was renamed, stops
// the build. Each page's Markdown is written beside its HTML.
// ponytail: substring search over the whole text (help.js); MiniSearch (MIT)
// once the help passes a few hundred pages or ranking starts to matter.
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, posix } from "node:path";
import { fileURLToPath } from "node:url";
import { Marked } from "marked";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..");
const out = join(here, "..", "public", "help");
const t0 = Date.now();
const read = (p) => readFileSync(join(root, p), "utf8").replace(/\r\n/g, "\n");
const VERSION = read("VERSION").trim();
const GITHUB = `https://github.com/Eyad-3D/simstudio/blob/v${VERSION}/`;
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const cell = (s) => String(s ?? "").replace(/\|/g, "\\|").replace(/\n/g, " ");
const SECTIONS = ["Get started", "Tutorials", "How-to guides", "Examples", "Reference", "Theory",
  "Validation", "Known issues", "Release notes", "Data sources", "Glossary"];

/** { sec, path: the page's .md path in the help, md, from: its source in the repo, ids } */
const pages = [];
const add = (sec, path, md, from = `docs/help/${path}`, ids = []) => pages.push({ sec, path, md, from, ids });

// A README section, from its heading to the next heading of the same or a
// higher level, with its headings moved up so that it becomes the page title.
const readme = read("README.md").split("\n");
function section(title) {
  let fenced = false;
  const level = (line) => (/^```/.test(line) ? ((fenced = !fenced), 0) : fenced ? 0 : /^(#+) /.exec(line)?.[1].length ?? 0);
  const start = readme.findIndex((l) => level(l) && l.replace(/^#+ /, "") === title);
  if (start < 0) throw new Error(`README.md has no section '${title}'`);
  const top = level(readme[start]);
  const body = [`# ${title}`];
  fenced = false;
  for (const line of readme.slice(start + 1)) {
    const h = level(line);
    if (h && h <= top) break;
    body.push(h ? "#".repeat(h - top + 1) + line.slice(h) : line);
  }
  return body.join("\n");
}

// ---- written for the help: docs/help/ -------------------------------------
const DIRS = { "": "Get started", tutorials: "Tutorials", "how-to": "How-to guides", reference: "Reference", theory: "Theory" };
for (const [dir, sec] of Object.entries(DIRS)) {
  const d = join(root, "docs", "help", dir);
  if (!existsSync(d)) continue;
  for (const f of readdirSync(d).filter((f) => f.endsWith(".md")).sort((a, b) => (b === "index.md") - (a === "index.md") || a.localeCompare(b))) {
    const path = posix.join(dir, f);
    add(f === "glossary.md" ? "Glossary" : sec, path, read(`docs/help/${path}`));
  }
}
add("Get started", "quick-start.md", section("Quick start"), "README.md");
add("Get started", "install.md", section("Install the desktop app"), "README.md");
add("Theory", "theory/solver.md", section("Solver"), "README.md");
add("Reference", "reference/file-format.md", section("Data model"), "README.md");
add("Reference", "reference/api.md", section("API"), "README.md");
// ---- the docs/ pages -------------------------------------------------------
for (const [sec, path, src] of [["Validation", "validation.md", "VALIDATION-STATUS.md"], ["Known issues", "known-limits.md", "KNOWN-LIMITS.md"],
  ["Release notes", "release-notes.md", "RELEASE-NOTES.md"], ["Data sources", "data-sources.md", "DATA-REGISTER.md"]])
  add(sec, path, read(`docs/${src}`), `docs/${src}`);
// ---- generated: examples, components, drive cycles -------------------------
for (const f of readdirSync(join(root, "backend", "projects")).filter((f) => f.endsWith(".json")).sort()) {
  const p = JSON.parse(read(`backend/projects/${f}`));
  const text = (p.description ?? "").split("\n").map((l) => l.replace(/^• /, "- ")).join("\n").replace(/^(?!- )(.+)$/gm, "$1\n");
  add("Examples", `examples/${p.id}.md`, `# ${p.name}\n\n${text}\n\n` +
    // an acceleration test ends at its line and a lap case on its track, not at its duration
    `Its cases: ${p.cases.map((c) => `*${c.name}* (${c.kind === "lap" ? "lap mode" : c.kind === "acceleration"
      ? `acceleration test over ${c.endDistance} m` : `${c.duration.toLocaleString("en")} s`})`).join(", ")}.\n\n` +
    "Open it from the *Start* page, under *New from an example*, or with **Open** on the *Home* tab. " +
    "It opens as a copy, so change it freely; **Save** keeps your copy as a project of your own.\n");
}
const lib = JSON.parse(read("backend/app/library/components.json"));
for (const c of lib.components) {
  const ports = c.ports.map((p) => `| ${cell(p.name)} | ${p.direction} | ${p.kind} | ${cell(p.unitGroup)} (${cell(lib.unitGroups[p.unitGroup] ?? "-")}) |`);
  // the Meaning column shows once the catalogue explains a parameter (LRN-05)
  const texts = c.parameters.map((p) => [p.description, p.options && `One of: ${p.options.join(", ")}.`,
    p.typical && `Typical: ${p.typical}`, p.whereToFind && `Where to find it: ${p.whereToFind}`].filter(Boolean).join(" "));
  const meaning = texts.some(Boolean);
  const params = c.parameters.map((p, i) => {
    const def = p.type.startsWith("table") ? "a table" : p.type === "code" ? "a script" : cell(p.default === "" ? "(none)" : p.default);
    return `| <span id="${p.key}"></span>${cell(p.label)} | ${cell(p.unit)} | ${def} |${meaning ? ` ${cell(texts[i])} |` : ""}`;
  });
  add("Reference", `reference/components/${c.id}.md`,
    `# ${c.name}\n\n${c.description ?? ""}\n\nGroup in the library: ${c.category}. Id: \`${c.id}\`.\n\n` +
    (ports.length ? `## Ports\n\n| Port | Direction | Kind | Quantity (unit) |\n|---|---|---|---|\n${ports.join("\n")}\n\n` : "") +
    (params.length ? `## Parameters\n\n| Parameter | Unit | Default |${meaning ? " Meaning |" : ""}\n|---|---|---|${meaning ? "---|" : ""}\n${params.join("\n")}\n` : ""),
    undefined, c.parameters.map((p) => p.key));
}
add("Reference", "reference/components/index.md", "# Components\n\nEvery part in the library, by its group. Select a part on the diagram and press F1 to open its page.\n\n" +
  [...new Set(lib.components.map((c) => c.category))].map((cat) => `## ${cat}\n\n` + lib.components.filter((c) => c.category === cat)
    .map((c) => `- [${c.name}](${c.id}.md): ${cell((c.description ?? "").split(/(?<=\.) /)[0])}`).join("\n")).join("\n\n"));
const cycles = JSON.parse(read("backend/app/cycles/cycles.json")).cycles;
add("Reference", "reference/drive-cycles.md", "# Drive cycles\n\nThe standard cycles LightSim includes. Pick one in a Driving Task's " +
  "**Drive Cycle** field ([how](../how-to/pick-a-drive-cycle.md)). Where the traces come from: [Data sources](../../DATA-REGISTER.md).\n\n" +
  "| Cycle | Id | Region | Duration | Distance | Phases |\n|---|---|---|---|---|---|\n" + Object.entries(cycles).map(([id, c]) =>
    `| ${c.name} | \`${id}\` | ${c.region} | ${c.published[0].toLocaleString("en")} s | ${c.published[1].toFixed(2)} km | ${c.phases.map((p) => p[0]).join(", ") || "none"} |`).join("\n") + "\n");

// ---- anchors: heading ids as GitHub makes them, so links match the repo ----
const slugger = () => {
  const seen = new Map();
  return (text) => {
    const s = text.toLowerCase().replace(/<[^>]+>/g, "").replace(/[^\p{L}\p{N}\s_-]/gu, "").trim().replace(/\s/g, "-");
    const n = seen.get(s) ?? 0;
    seen.set(s, n + 1);
    return n ? `${s}-${n}` : s;
  };
};
const marked = new Marked({ gfm: true });
const byPath = new Map(pages.map((p) => [p.path, p]));
const bySource = new Map(pages.filter((p) => p.from !== "README.md").map((p) => [p.from, p.path]));
for (const p of pages) {
  const slug = slugger();
  p.anchors = new Set(p.ids);
  marked.walkTokens(marked.lexer(p.md), (t) => t.type === "heading" && p.anchors.add(slug(t.text)));
}

// ---- links: pages and headings in the help; other repo files on GitHub ----
const broken = [];
const images = new Map();
let page;
function resolve(tok) {
  if (/^[a-z][a-z\d+.-]*:/i.test(tok.href)) return;
  const [target, frag] = tok.href.split("#");
  const abs = target ? posix.normalize(posix.join(posix.dirname(page.from), target)) : page.from;
  let to = abs === page.from ? page.path : bySource.get(abs);
  // a README heading that one of its sections' pages holds
  if (abs === "README.md") to = pages.find((q) => q.from === "README.md" && q.anchors.has(frag))?.path;
  if (tok.type === "image" && existsSync(join(root, abs))) {
    images.set(`images/${posix.basename(abs)}`, abs);
    tok.href = posix.relative(posix.dirname(page.path), `images/${posix.basename(abs)}`);
  } else if (to && (!frag || byPath.get(to).anchors.has(frag))) {
    tok.href = (to === page.path && frag ? "" : posix.relative(posix.dirname(page.path), to).replace(/\.md$/, ".html")) + (frag ? `#${frag}` : "");
  } else if (!to && existsSync(join(root, abs)) && !abs.startsWith("..")) {
    tok.href = GITHUB + abs + (frag ? `#${frag}` : "");
  } else broken.push(`${page.from} → ${tok.href}`);
}
let slug;
marked.use({
  walkTokens: (tok) => (tok.type === "link" || tok.type === "image") && resolve(tok),
  renderer: { heading({ tokens, depth, text }) { return `<h${depth} id="${slug(text)}">${this.parser.parseInline(tokens)}</h${depth}>\n`; } },
});

// ---- render ----------------------------------------------------------------
rmSync(out, { recursive: true, force: true });
const title = (md) => /^# (.+)$/m.exec(md)?.[1] ?? "Untitled";
const inNav = (p) => !p.path.startsWith("reference/components/") || p.path.endsWith("index.md");
const used = SECTIONS.filter((s) => pages.some((p) => p.sec === s));
const index = [];
for (page of pages) {
  slug = slugger();
  const h1 = slug(title(page.md));
  const body = marked.parse(page.md.replace(/^# .+\n/, ""));
  const up = "../".repeat(page.path.split("/").length - 1);
  const nav = used.map((sec) => `<h2>${sec}</h2><ul>${pages.filter((q) => q.sec === sec && inNav(q)).map((q) =>
    `<li><a href="${up}${q.path.replace(/\.md$/, ".html")}"${q === page ? ' aria-current="page"' : ""}>${esc(title(q.md))}</a></li>`).join("")}</ul>`).join("");
  const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title(page.md))} · LightSim ${VERSION} help</title><link rel="stylesheet" href="${up}help.css"></head><body>
<a class="skip" href="#main">Skip to the page</a>
<nav aria-label="Help pages"><p class="brand"><a href="${up}index.html">LightSim ${VERSION} help</a></p>
<div role="search"><label for="q">Search the help</label><input id="q" type="search" autocomplete="off"><ul id="hits" aria-live="polite"></ul></div>${nav}</nav>
<main id="main"><h1 id="${h1}">${esc(title(page.md))}</h1>
${body}</main>
<footer>LightSim ${VERSION}</footer><script src="${up}search-index.js"></script><script src="${up}help.js"></script></body></html>
`;
  const file = join(out, page.path.replace(/\.md$/, ".html"));
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, html);
  writeFileSync(join(out, page.path), page.md);
  index.push({ u: page.path.replace(/\.md$/, ".html"), t: title(page.md), s: page.sec, x: body.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim() });
}
if (broken.length) throw new Error(`Broken links in the help:\n${broken.join("\n")}`);
mkdirSync(join(out, "images"), { recursive: true });
for (const [to, from] of images) copyFileSync(join(root, from), join(out, to));
writeFileSync(join(out, "search-index.js"), `window.HELP_INDEX=${JSON.stringify(index)};\n`);
for (const f of ["help.js", "help.css"]) copyFileSync(join(here, "help-assets", f), join(out, f));
console.log(`help: ${pages.length} pages in ${used.length} sections, LightSim ${VERSION}, ${Date.now() - t0} ms → ${out}`);
