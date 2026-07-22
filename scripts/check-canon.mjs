#!/usr/bin/env node
// Canon spine lint. Enforces the doc spine and link invariants specified in
// prose/README.md ("Canon doc spine"): every canon doc opens Title → anchor
// blockquote (line 3, with an **Implementation** line pointing at a folder,
// never a file) → `## TL;DR`; canon never links into proposals/ or plans/;
// references never link out to other prose docs. Zero dependencies.
//
// Usage: node scripts/check-canon.mjs
import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

const problems = [];
const fail = (file, msg) => problems.push(`${file}: ${msg}`);

// A path with a source-file extension inside an Implementation anchor.
const FILE_IN_ANCHOR = /[\w/-]+\.(?:rs|m?[jt]s|py|typ|md|ya?ml|toml|json|sh|wasm)\b/;
// A markdown link target into the proposal/plan tiers.
const PLAN_LINK = /\]\([^)]*\b(?:proposals|plans)\//;
// A relative markdown link target to a .md file (an outbound prose link).
const PROSE_LINK = /\]\((?!https?:)[^)]*\.md(?=[)#])/;

const mdFiles = (dir) =>
  existsSync(dir) ? readdirSync(dir).filter((n) => n.endsWith('.md')).sort() : [];

for (const name of mdFiles('prose/canon')) {
  const file = join('prose/canon', name);
  const text = readFileSync(file, 'utf8');
  const lines = text.split('\n');

  const planLink = text.match(PLAN_LINK);
  if (planLink) fail(file, `links into proposals/ or plans/ (\`${planLink[0]}\`) — canon never references them`);

  if (name === 'INDEX.md') continue; // the index has no spine

  if (!lines[0]?.startsWith('# ')) fail(file, 'line 1 is not a `# Title`');

  // Anchor blockquote: contiguous `>` lines from line 3. The **Implementation**
  // entry is its line plus continuation lines up to the next `> **Key**:` line.
  if (!lines[2]?.startsWith('> ')) {
    fail(file, 'line 3 is not the anchor blockquote');
  } else {
    let quote = [];
    for (let i = 2; i < lines.length && lines[i].startsWith('>'); i++) quote.push(lines[i]);
    const start = quote.findIndex((l) => l.startsWith('> **Implementation**:'));
    if (start === -1) {
      fail(file, 'anchor blockquote has no `> **Implementation**:` line');
    } else {
      let impl = quote[start];
      for (let i = start + 1; i < quote.length && !/^> \*\*\w+\*\*:/.test(quote[i]); i++) impl += '\n' + quote[i];
      const m = impl.match(FILE_IN_ANCHOR);
      if (m) fail(file, `Implementation anchor names a file (\`${m[0]}\`) — anchors point at folders or modules`);
    }
  }

  const firstH2 = lines.find((l) => l.startsWith('## '));
  if (firstH2 !== '## TL;DR') fail(file, `first section is \`${firstH2 ?? '(none)'}\` — canon docs open with \`## TL;DR\``);
}

for (const name of mdFiles('prose/references')) {
  const file = join('prose/references', name);
  const m = readFileSync(file, 'utf8').match(PROSE_LINK);
  if (m) fail(file, `links to another prose doc (\`${m[0]}\`) — references are self-contained`);
}

if (problems.length) {
  for (const p of problems) console.error(`check-canon: ${p}`);
  process.exit(1);
}
console.log('check-canon: canon spine OK');
