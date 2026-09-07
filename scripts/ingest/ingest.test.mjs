//! Tests for the parts of the crawl that can be wrong without failing.
//!
//! An HTTP error is loud. A build path assembled in the wrong order, a win
//! rate double-counted on merge, or a champion filed under the wrong role are
//! silent — they produce a file that parses, passes the output guard, ships,
//! and is wrong. Those are what this covers.
//!
//!   node --test scripts/ingest/
//!
//! No dependencies: Node's own runner, matching the rest of the crawler.

import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { parseLimits, regionFor } from "./api.mjs";
import { isOnPatch, patchOf } from "./catalog.mjs";
import {
  addSample,
  emptyTally,
  maxOrder,
  purchasesByParticipant,
  roleOf,
  samplesFromMatch,
  tallyToFile,
} from "./aggregate.mjs";
import { mergeBuildFiles, mergeCounted } from "./store.mjs";

/* ---------- a stand-in catalog ---------- */

const ITEMS = {
  3340: { id: 3340, cost: 0, trinket: true, consumable: false, boots: false, finished: false },
  2003: { id: 2003, cost: 50, trinket: false, consumable: true, boots: false, finished: false },
  1056: { id: 1056, cost: 400, trinket: false, consumable: false, boots: false, finished: false },
  3020: { id: 3020, cost: 1100, trinket: false, consumable: false, boots: true, finished: false },
  3165: { id: 3165, cost: 2850, trinket: false, consumable: false, boots: false, finished: true },
  4645: { id: 4645, cost: 3000, trinket: false, consumable: false, boots: false, finished: true },
  3157: { id: 3157, cost: 3250, trinket: false, consumable: false, boots: false, finished: true },
  6653: { id: 6653, cost: 3200, trinket: false, consumable: false, boots: false, finished: true },
};

const CHAMPIONS = {
  103: { key: "Ahri", name: "Ahri", id: 103 },
  62: { key: "MonkeyKing", name: "Wukong", id: 62 },
};

const catalog = {
  patch: "16.17",
  champion: (id) => CHAMPIONS[id] ?? null,
  item: (id) => ITEMS[id] ?? null,
};

const purchase = (participantId, itemId, timestamp) => ({
  type: "ITEM_PURCHASED",
  participantId,
  itemId,
  timestamp,
});

const skillUp = (participantId, skillSlot) => ({ type: "SKILL_LEVEL_UP", participantId, skillSlot });

const timelineOf = (events) => ({ info: { frames: [{ events }] } });

/* ---------- routing and limits ---------- */

test("match-v5 is routed by region, not by platform", () => {
  assert.equal(regionFor("euw1"), "europe");
  assert.equal(regionFor("na1"), "americas");
  assert.equal(regionFor("kr"), "asia");
  assert.throws(() => regionFor("not-a-platform"), /unknown platform/);
});

test("the documented development limits parse, and nonsense does not", () => {
  const windows = parseLimits("20:1,100:120");
  assert.equal(windows.length, 2);
  assert.equal(windows[1].count, 100);
  assert.equal(windows[1].windowMs, 120_000);
  assert.throws(() => parseLimits("100"), /bad rate limit/);
  assert.throws(() => parseLimits("0:120"), /bad rate limit/);
});

test("a window refuses a request once it is full and frees up on its own", () => {
  const [window] = parseLimits("2:10");
  const now = 1_000_000;
  assert.equal(window.waitMs(now), 0);
  window.record(now);
  window.record(now);
  assert.ok(window.waitMs(now) > 0, "a full window must make the caller wait");
  assert.equal(window.waitMs(now + 10_001), 0, "and let go once the window passes");
});

/* ---------- patches ---------- */

test("patches compare at major.minor, so a hotfix does not discard the sample", () => {
  assert.equal(patchOf("16.17.1"), "16.17");
  assert.ok(isOnPatch("16.17.681.9271", "16.17"));
  assert.ok(!isOnPatch("16.16.612.1234", "16.17"), "last patch is not this patch");
});

/* ---------- reading a match ---------- */

test("only the five real positions count as a role", () => {
  assert.equal(roleOf({ teamPosition: "MIDDLE" }), "middle");
  assert.equal(roleOf({ teamPosition: "UTILITY" }), "utility");
  assert.equal(roleOf({ teamPosition: "" }), null, "an unassigned position is not a role");
  assert.equal(roleOf({ teamPosition: "AFK" }), null);
  assert.equal(roleOf({}), null);
});

test("an undone purchase is not part of the build", () => {
  const purchases = purchasesByParticipant(
    timelineOf([
      purchase(1, 1056, 0),
      purchase(1, 3157, 500_000),
      { type: "ITEM_UNDO", participantId: 1, beforeId: 3157, afterId: 0 },
      purchase(1, 3165, 600_000),
    ]),
  );
  assert.deepEqual(
    purchases.get(1).map((entry) => entry.itemId),
    [1056, 3165],
    "the item the player took back is still in the build path",
  );
});

test("an undo removes the most recent copy, not the first", () => {
  const purchases = purchasesByParticipant(
    timelineOf([
      purchase(1, 2003, 0),
      purchase(1, 2003, 100),
      { type: "ITEM_UNDO", participantId: 1, beforeId: 2003, afterId: 0 },
    ]),
  );
  assert.equal(purchases.get(1).length, 1);
});

test("max order ranks by which ability reached five points first", () => {
  // Q maxed at level 9, then E.
  const slots = [1, 3, 1, 2, 1, 4, 1, 3, 1, 4, 3, 3, 3, 4, 2, 2, 2, 2];
  assert.deepEqual(maxOrder(slots), ["Q", "E", "W"]);
});

test("a game that ended early still says which way the player was going", () => {
  // Nothing is maxed. Q has the most points, then E.
  assert.deepEqual(maxOrder([1, 3, 1, 2, 1, 3]), ["Q", "E", "W"]);
  assert.deepEqual(maxOrder([]), [], "and a game with no level-ups says nothing");
});

test("a match becomes one sample per usable participant", () => {
  const match = {
    info: {
      participants: [
        {
          participantId: 1,
          championId: 103,
          teamPosition: "MIDDLE",
          win: true,
          summoner1Id: 4,
          summoner2Id: 14,
          perks: {
            statPerks: { offense: 5008, flex: 5008, defense: 5001 },
            styles: [
              { description: "primaryStyle", style: 8200, selections: [{ perk: 8214 }, { perk: 8226 }] },
              { description: "subStyle", style: 8300, selections: [{ perk: 8347 }] },
            ],
          },
        },
        // No assigned position: dropped rather than guessed at.
        { participantId: 2, championId: 62, teamPosition: "", win: false },
        // A champion the catalog has never heard of: also dropped.
        { participantId: 3, championId: 99_999, teamPosition: "TOP", win: false },
      ],
    },
  };

  const timeline = timelineOf([
    purchase(1, 1056, 0),
    purchase(1, 2003, 1_000),
    purchase(1, 3340, 2_000), // trinket: free, and nobody chooses it
    purchase(1, 3020, 400_000), // boots
    purchase(1, 3165, 700_000),
    purchase(1, 4645, 1_200_000),
    purchase(1, 3157, 1_600_000),
    purchase(1, 6653, 2_000_000), // a fourth finished item: situational
    skillUp(1, 1),
    skillUp(1, 3),
    skillUp(1, 1),
  ]);

  const samples = samplesFromMatch(match, timeline, catalog);
  assert.equal(samples.length, 1, "only the participant we can read anything about");

  const [sample] = samples;
  assert.equal(sample.championKey, "Ahri");
  assert.equal(sample.role, "middle");
  assert.equal(sample.win, true);
  assert.deepEqual(sample.starters, [1056, 2003], "trinkets are not a start");
  assert.equal(sample.boots, 3020);
  assert.deepEqual(sample.core, [3165, 4645, 3157], "in purchase order, boots excluded");
  assert.deepEqual(sample.situational, [6653]);
  assert.deepEqual(sample.summoners, [4, 14]);
  assert.deepEqual(sample.runes.primary, [8214, 8226]);
  assert.deepEqual(sample.runes.shards, [5008, 5008, 5001], "offence, flex, defence");
  assert.deepEqual(sample.skillOrder, ["Q", "E", "Q"]);
});

test("a participant who never finished an item contributes nothing", () => {
  const match = {
    info: {
      participants: [{ participantId: 1, championId: 103, teamPosition: "MIDDLE", win: false }],
    },
  };
  const timeline = timelineOf([purchase(1, 1056, 0), purchase(1, 3020, 300_000)]);
  assert.deepEqual(samplesFromMatch(match, timeline, catalog), []);
});

/* ---------- the file ---------- */

function tallyOf(samples) {
  const tally = emptyTally();
  for (const sample of samples) addSample(tally, sample);
  return [...tally.values()][0];
}

const sampleOf = (overrides = {}) => ({
  championId: 103,
  championKey: "Ahri",
  championName: "Ahri",
  role: "middle",
  win: true,
  starters: [1056, 2003],
  boots: 3020,
  core: [3165, 4645, 3157],
  situational: [6653],
  runes: { primaryStyle: 8200, primary: [8214], secondaryStyle: 8300, secondary: [8347], shards: [5008] },
  summoners: [4, 14],
  skillOrder: ["Q", "E", "Q"],
  skillPriority: ["Q", "E", "W"],
  ...overrides,
});

test("the file matches the shape the app reads", () => {
  const entry = tallyOf([sampleOf(), sampleOf({ win: false }), sampleOf({ win: true })]);
  const file = tallyToFile(entry, {
    patch: "16.17",
    region: "na1",
    tier: "challenger",
    updatedAt: "2026-09-05T00:00:00.000Z",
  });

  assert.equal(file.schemaVersion, 1);
  assert.deepEqual(file.champion, { key: "Ahri", name: "Ahri", id: 103 });
  assert.equal(file.role, "middle");
  assert.equal(file.stats.games, 3);
  assert.equal(file.stats.winRate, 0.6667);

  // Group stats are flattened into the group, not nested under it — that is
  // the contract with FileItemGroup in riot/file.rs.
  const [core] = file.items.core;
  assert.deepEqual(core.items, [3165, 4645, 3157]);
  assert.equal(core.games, 3);
  assert.equal(core.winRate, 0.6667);
  assert.equal(core.stats, undefined, "stats must be flattened, not nested");

  assert.deepEqual(file.skills.priority, ["Q", "E", "W"]);
  assert.deepEqual(file.summoners[0].spells, [4, 14]);
  assert.equal(file.runes[0].primaryStyle, 8200);
});

test("the most played path comes first", () => {
  const entry = tallyOf([
    sampleOf({ core: [3165, 4645, 3157] }),
    sampleOf({ core: [6653, 3165, 3157] }),
    sampleOf({ core: [6653, 3165, 3157] }),
  ]);
  const file = tallyToFile(entry, { patch: "16.17" });
  assert.deepEqual(file.items.core[0].items, [6653, 3165, 3157]);
  assert.equal(file.items.core[0].games, 2);
});

/* ---------- accumulating ---------- */

test("a second run adds to the first rather than replacing it", () => {
  const yesterday = {
    schemaVersion: 1,
    patch: "16.17",
    stats: { games: 100, winRate: 0.5 },
    items: { starters: [], boots: [], core: [{ items: [1, 2, 3], games: 100, winRate: 0.5 }], situational: [] },
    runes: [],
    summoners: [],
    skills: { priority: ["Q"], order: [] },
  };
  const today = {
    schemaVersion: 1,
    patch: "16.17",
    stats: { games: 20, winRate: 1 },
    items: { starters: [], boots: [], core: [{ items: [1, 2, 3], games: 20, winRate: 1 }], situational: [] },
    runes: [],
    summoners: [],
    skills: { priority: ["Q", "E"], order: [] },
  };

  const merged = mergeBuildFiles(yesterday, today);
  assert.equal(merged.stats.games, 120);
  assert.equal(merged.stats.winRate, 0.5833, "fifty wins plus twenty, over a hundred and twenty");
  assert.equal(merged.items.core[0].games, 120);
  assert.deepEqual(merged.skills.priority, ["Q", "E"], "the fresher answer wins for skills");
});

test("a new patch throws the old sample away instead of averaging across it", () => {
  const lastPatch = {
    schemaVersion: 1,
    patch: "16.16",
    stats: { games: 5000, winRate: 0.5 },
    items: { core: [{ items: [9, 9, 9], games: 5000, winRate: 0.5 }] },
  };
  const thisPatch = {
    schemaVersion: 1,
    patch: "16.17",
    stats: { games: 12, winRate: 0.6 },
    items: { core: [{ items: [1, 2, 3], games: 12, winRate: 0.6 }] },
  };

  const merged = mergeBuildFiles(lastPatch, thisPatch);
  assert.equal(merged.stats.games, 12, "five thousand games from a game nobody is playing");
  assert.deepEqual(merged.items.core[0].items, [1, 2, 3]);
});

test("merging keeps only the most played, so files do not grow forever", () => {
  const existing = [
    { items: [1], games: 10, winRate: 0.5 },
    { items: [2], games: 5, winRate: 0.4 },
  ];
  const fresh = [
    { items: [2], games: 50, winRate: 0.6 },
    { items: [3], games: 1, winRate: 1 },
  ];

  const merged = mergeCounted(existing, fresh, (group) => group.items.join("-"), 2);
  assert.equal(merged.length, 2);
  assert.deepEqual(merged[0].items, [2]);
  assert.equal(merged[0].games, 55, "the same path from both runs is one entry");
  assert.equal(merged[0].winRate, 0.5818);
  assert.deepEqual(merged[1].items, [1]);
});

test("merging is stable, so an unchanged crawl produces an unchanged file", () => {
  const list = [
    { items: [1], games: 10, winRate: 0.5 },
    { items: [2], games: 10, winRate: 0.5 },
  ];
  const once = mergeCounted(list, [], (group) => group.items.join("-"), 5);
  const twice = mergeCounted(list, [], (group) => group.items.join("-"), 5);
  assert.deepEqual(once, twice);
});

/* ---------- the guard actually accepts what we write ---------- */

test("output written by the crawler passes the commit guard", () => {
  const dir = mkdtempSync(join(tmpdir(), "spellcheck-ingest-"));
  try {
    const entry = tallyOf([sampleOf(), sampleOf({ win: false })]);
    const file = tallyToFile(entry, {
      patch: "16.17",
      region: "na1",
      tier: "challenger",
      updatedAt: new Date().toISOString(),
    });

    mkdirSync(join(dir, "Ahri"), { recursive: true });
    writeFileSync(join(dir, "Ahri", "middle.json"), `${JSON.stringify(file, null, 2)}\n`);

    const output = execFileSync(
      process.execPath,
      ["scripts/ingest/verify-output.mjs", dir, "--allow-shrink"],
      { encoding: "utf8" },
    );
    assert.match(output, /Safe to commit/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
