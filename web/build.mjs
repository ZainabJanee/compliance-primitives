import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(new URL("..", import.meta.url).pathname);
const web = resolve(root, "web");
const escapeHtml = (value) => value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/\"/g, "&quot;");
const shell = (title, description, body) => `<!doctype html>\n<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${title} — compliance-primitives</title><meta name="description" content="${description}"><link rel="icon" href="favicon.svg" type="image/svg+xml"><link rel="stylesheet" href="styles.css"><link rel="alternate" type="application/atom+xml" title="compliance-primitives changelog" href="feed.xml"></head><body><nav><div class="wrap"><a class="brand" href="index.html">compliance-primitives</a><div class="nav-links"><a href="index.html#contracts">Contracts</a><a href="architecture.html">Architecture</a><a href="changelog.html">Changelog</a><a href="index.html">Home</a></div></div></nav>${body}<footer><div class="wrap"><div class="foot-row"><span>MIT License · Stellar compliance primitives</span><a href="https://github.com/stellar-compliance-kit/compliance-primitives">GitHub</a></div></div></footer></body></html>`;
const changelog = await readFile(resolve(root, "CHANGELOG.md"), "utf8");
const architecture = await readFile(resolve(root, "ARCHITECTURE.md"), "utf8");
await writeFile(resolve(web, "changelog.html"), shell("Changelog", "Release history for compliance-primitives", `<main class="page wrap"><span class="badge">Release history</span><h1>Changelog</h1><p class="lede">A build-generated view of the repository changelog. <a href="feed.xml" type="application/atom+xml">Subscribe via Atom feed</a>.</p><article class="markdown"><pre>${escapeHtml(changelog)}</pre></article></main>`));
await writeFile(resolve(web, "architecture.html"), shell("Architecture", "Composition graph for the nine compliance-primitives contracts", `<main class="page wrap"><span class="badge">System design</span><h1>Contract architecture</h1><p class="lede">Nine focused Soroban contracts composed through explicit checks and administrative controls.</p><div class="diagram"><img src="architecture.svg" alt="Diagram showing the composition relationships between nine compliance contracts"></div><article class="markdown"><h2>Source architecture</h2><pre>${escapeHtml(architecture)}</pre></article></main>`));

// ── Atom feed generation ──────────────────────────────────────────────────────
const SITE_URL = "https://stellar-compliance-kit.github.io/compliance-primitives";

/**
 * Parse CHANGELOG.md into a list of { version, date, content } entries.
 * Sections start with `## [x.y.z] — YYYY-MM-DD` or `## [Unreleased]`.
 */
function parseChangelog(md) {
  const entries = [];
  const lines = md.split("\n");
  let current = null;
  for (const line of lines) {
    const m = line.match(/^## \[([^\]]+)\](?:\s*[—–-]\s*(\d{4}-\d{2}-\d{2}))?/);
    if (m) {
      if (current) entries.push(current);
      current = { version: m[1], date: m[2] ?? null, lines: [] };
    } else if (current) {
      current.lines.push(line);
    }
  }
  if (current) entries.push(current);
  return entries.map((e) => ({ ...e, content: e.lines.join("\n").trim() }));
}

function atomDate(dateStr) {
  // Atom requires RFC 3339; fall back to build time for undated entries.
  return dateStr ? `${dateStr}T00:00:00Z` : new Date().toISOString();
}

const entries = parseChangelog(changelog);
const updated = entries.find((e) => e.date)?.date
  ? atomDate(entries.find((e) => e.date).date)
  : new Date().toISOString();

const feedEntries = entries.map((e) => `
  <entry>
    <title>${escapeHtml(`${e.version === "Unreleased" ? "Unreleased" : `v${e.version}`} — compliance-primitives`)}</title>
    <link href="${SITE_URL}/changelog.html#${e.version.toLowerCase().replace(/\./g, "-")}"/>
    <id>${SITE_URL}/changelog.html#${e.version.toLowerCase().replace(/\./g, "-")}</id>
    <updated>${atomDate(e.date)}</updated>
    <content type="text">${escapeHtml(e.content || "(no details recorded)")}</content>
  </entry>`).join("\n");

const feed = `<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>compliance-primitives changelog</title>
  <link href="${SITE_URL}/changelog.html"/>
  <link rel="self" href="${SITE_URL}/feed.xml" type="application/atom+xml"/>
  <id>${SITE_URL}/feed.xml</id>
  <updated>${updated}</updated>
  <author><name>Stellar Compliance Kit contributors</name></author>
${feedEntries}
</feed>
`;

await writeFile(resolve(web, "feed.xml"), feed);
console.log("Generated feed.xml");

