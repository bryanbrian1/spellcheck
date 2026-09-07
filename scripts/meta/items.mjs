#!/usr/bin/env node
// Regenerate data/meta/items.json from Data Dragon.
//
// Two different kinds of thing live in that file and this script is careful to
// keep them apart.
//
// `answers` and `damage` are judgement — which item answers which threat, and
// whether an item is physical or magical. They are curated, they are the whole
// reason the file exists, and this script NEVER invents them. It reads the
// committed file and carries every existing entry across untouched.
//
// Everything else — name, cost, buildsInto, isComponent — is mechanical, comes
// from Data Dragon, and is regenerated wholesale.
//
// The file used to hold only the 75 items that answer something. It now holds
// the whole catalogue, because the in-game gold comparison needs a cost for
// whatever a player is actually carrying, and the live API cannot supply one:
// its `price` field is the *combine* cost, so summing it counts only the last
// step of each build path. See docs/roadmap.html, defect 1.
//
//   node scripts/meta/items.mjs          # rewrite the file
//   node scripts/meta/items.mjs --check  # fail if it would change (for CI)

import { readFileSync, writeFileSync } from "node:fs";

const OUT = new URL("../../data/meta/items.json", import.meta.url);
const DDRAGON = "https://ddragon.leagueoflegends.com";
const SCHEMA_VERSION = 1;

const NOTE =
  "Every item Data Dragon knows, with the cost the app compares spend against. " +
  "`cost` is Data Dragon's gold.total — the whole build path — because the " +
  "live game API reports only gold.base and summing that undercounts a " +
  "finished item by roughly three quarters. `answers` and `damage` are " +
  "curated judgement and are never generated; every other field is derived " +
  "from Data Dragon and rewritten by scripts/meta/items.mjs.";

const get = async (url) => {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url} answered ${response.status}`);
  return response.json();
};

// Everything Data Dragon lists goes in, with no filter, and that is a
// deliberate correction rather than laziness.
//
// The obvious filter is `gold.purchasable`, and it is wrong here. A player
// carries plenty of things they never bought from the shop — Eye of the
// Herald, the support quest upgrades, elixirs, biscuits — and every one of
// them showed up in a captured game while being absent from a purchasable-only
// table. This table answers "what is the thing in that slot worth", not "what
// can you buy", so the only safe catalogue is the whole one. An item missing
// here becomes an unknown at runtime and suppresses the comparison entirely,
// which is a high price for a filter that saves a few kilobytes.

const main = async () => {
  const check = process.argv.includes("--check");
  const existing = JSON.parse(readFileSync(OUT, "utf8"));

  const versions = await get(`${DDRAGON}/api/versions.json`);
  const version = versions[0];
  const { data } = await get(`${DDRAGON}/cdn/${version}/data/en_US/item.json`);

  // Which items build into which. Data Dragon records the relationship on the
  // child ("from"), and the app asks it in the other direction.
  const buildsInto = new Map();
  for (const [id, item] of Object.entries(data)) {
    for (const component of item.from ?? []) {
      if (!buildsInto.has(component)) buildsInto.set(component, []);
      buildsInto.get(component).push(Number(id));
    }
  }

  const items = {};
  for (const [id, item] of Object.entries(data)) {
    const carried = existing.items[id];
    const into = (buildsInto.get(id) ?? []).sort((a, b) => a - b);
    items[id] = {
      name: item.name,
      // gold.total, never gold.base. This is the entire point of the file.
      cost: item.gold?.total ?? 0,
      answers: carried?.answers ?? [],
      damage: carried?.damage ?? null,
      buildsInto: into,
      isComponent: into.length > 0,
    };
  }

  // A curated entry that no longer exists in Data Dragon is a real problem —
  // some check still reaches for it and will now find nothing. Say so loudly
  // rather than dropping it silently.
  const lostJudgement = Object.entries(existing.items)
    .filter(([id, tags]) => !items[id] && (tags.answers?.length || tags.damage))
    .map(([id, tags]) => `${id} (${tags.name})`);
  if (lostJudgement.length) {
    console.error(
      `refusing to drop ${lostJudgement.length} curated item(s) missing from ` +
        `Data Dragon ${version}:\n  ${lostJudgement.join("\n  ")}`,
    );
    process.exit(1);
  }

  const sorted = Object.fromEntries(
    Object.entries(items).sort(([a], [b]) => Number(a) - Number(b)),
  );
  const next = {
    schemaVersion: SCHEMA_VERSION,
    note: NOTE,
    patch: version,
    items: sorted,
  };
  const text = `${JSON.stringify(next, null, 2)}\n`;

  if (check) {
    const current = readFileSync(OUT, "utf8");
    if (current !== text) {
      console.error("data/meta/items.json is stale; run node scripts/meta/items.mjs");
      process.exit(1);
    }
    console.log(`items.json is current for ${version}`);
    return;
  }

  writeFileSync(OUT, text);
  const curated = Object.values(sorted).filter((i) => i.answers.length).length;
  console.log(
    `wrote ${Object.keys(sorted).length} items for ${version} ` +
      `(${curated} carry curated answers)`,
  );
};

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
