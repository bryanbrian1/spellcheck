//! One whole run, against a stubbed Riot.
//!
//! aggregate.mjs is tested on its own, but the orchestration around it is
//! where the quiet mistakes live: a wrong URL path, a response read at the
//! wrong nesting, a filter that drops every match. None of those would throw
//! — they produce a run that reports success and writes nothing.
//!
//! So `fetch` is replaced with a router that answers like Riot does, the real
//! `crawl()` is driven end to end, and the files it writes are checked
//! against the real commit guard.

import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const PATCH = "16.17";
const VERSION = `${PATCH}.1`;

/* ---------- what the stub serves ---------- */

const CHAMPIONS = {
  Ahri: { key: "103", name: "Ahri" },
  MonkeyKing: { key: "62", name: "Wukong" },
  Leona: { key: "89", name: "Leona" },
};

const ITEMS = {
  1056: { name: "Doran's Ring", gold: { total: 400, purchasable: true }, into: [], tags: [] },
  2003: { name: "Health Potion", gold: { total: 50, purchasable: true }, into: [], tags: ["Consumable"], consumed: true },
  3340: { name: "Stealth Ward", gold: { total: 0, purchasable: true }, into: [], tags: [] },
  3020: { name: "Sorcerer's Shoes", gold: { total: 1100, purchasable: true }, into: [], tags: ["Boots"] },
  3165: { name: "Morellonomicon", gold: { total: 2850, purchasable: true }, into: [], tags: [] },
  4645: { name: "Shadowflame", gold: { total: 3000, purchasable: true }, into: [], tags: [] },
  3157: { name: "Zhonya's Hourglass", gold: { total: 3250, purchasable: true }, into: [], tags: [] },
};

const PARTICIPANTS = [
  { championId: 103, teamPosition: "MIDDLE" },
  { championId: 62, teamPosition: "JUNGLE" },
  { championId: 89, teamPosition: "UTILITY" },
];

const matchFor = (id) => ({
  info: {
    gameVersion: `${PATCH}.681.9271`,
    queueId: 420,
    gameDuration: 1800,
    gameEndTimestamp: 1_700_000_000_000,
    participants: PARTICIPANTS.map((base, index) => ({
      ...base,
      participantId: index + 1,
      // Alternate wins between matches so the rate is not 0 or 1.
      win: (Number(id.slice(1)) + index) % 2 === 0,
      summoner1Id: 4,
      summoner2Id: 14,
      perks: {
        statPerks: { offense: 5008, flex: 5008, defense: 5001 },
        styles: [
          { description: "primaryStyle", style: 8200, selections: [{ perk: 8214 }] },
          { description: "subStyle", style: 8300, selections: [{ perk: 8347 }] },
        ],
      },
    })),
  },
});

const timelineFor = () => ({
  info: {
    frames: [
      {
        events: PARTICIPANTS.flatMap((_, index) => {
          const p = index + 1;
          return [
            { type: "ITEM_PURCHASED", participantId: p, itemId: 1056, timestamp: 0 },
            { type: "ITEM_PURCHASED", participantId: p, itemId: 3340, timestamp: 100 },
            { type: "ITEM_PURCHASED", participantId: p, itemId: 3020, timestamp: 400_000 },
            { type: "ITEM_PURCHASED", participantId: p, itemId: 3165, timestamp: 700_000 },
            { type: "ITEM_PURCHASED", participantId: p, itemId: 4645, timestamp: 1_200_000 },
            { type: "ITEM_PURCHASED", participantId: p, itemId: 3157, timestamp: 1_600_000 },
            { type: "SKILL_LEVEL_UP", participantId: p, skillSlot: 1 },
            { type: "SKILL_LEVEL_UP", participantId: p, skillSlot: 3 },
            { type: "SKILL_LEVEL_UP", participantId: p, skillSlot: 1 },
          ];
        }),
      },
    ],
  },
});

/** Answers like Riot and Data Dragon do, and records what was asked. */
function stubFetch(asked) {
  const json = (body) => ({ ok: true, status: 200, json: async () => body, headers: new Map() });

  return async (url) => {
    const path = String(url);
    asked.push(path);

    if (path.endsWith("/api/versions.json")) return json([VERSION, "16.16.1"]);
    if (path.includes("/data/en_US/champion.json")) return json({ data: CHAMPIONS });
    if (path.includes("/data/en_US/item.json")) return json({ data: ITEMS });

    if (path.includes("/lol/league/v4/challengerleagues/")) {
      return json({ entries: [{ puuid: "puuid-a" }, { puuid: "puuid-b" }] });
    }
    if (path.includes("/lol/league/v4/grandmasterleagues/")) {
      return json({ entries: [{ puuid: "puuid-c" }] });
    }
    if (path.includes("/ids?")) return json(["M1", "M2", "M3"]);

    const timeline = path.match(/\/matches\/(\w+)\/timeline$/);
    if (timeline) return json(timelineFor());

    const match = path.match(/\/matches\/(\w+)$/);
    if (match) return json(matchFor(match[1]));

    return { ok: false, status: 404, json: async () => ({}), headers: new Map() };
  };
}

/* ---------- the run ---------- */

test("a whole crawl reaches the disk, and the guard accepts what it wrote", async () => {
  const out = mkdtempSync(join(tmpdir(), "leaguechecker-crawl-"));
  const realFetch = globalThis.fetch;
  const asked = [];
  globalThis.fetch = stubFetch(asked);

  process.env.RIOT_API_KEY = "RGAPI-stub";
  process.env.INGEST_OUT = out;
  process.env.INGEST_MINUTES = "5";
  process.env.INGEST_MIN_GAMES = "1";
  process.env.RIOT_RATE_LIMITS = "1000:1";

  try {
    // Imported fresh so the module reads the environment set above.
    const { crawl } = await import(`./index.mjs?case=${Date.now()}`);
    const result = await crawl();

    assert.equal(result.ok, true, result.why);
    assert.equal(result.counts.matches, 3, "three distinct matches, deduplicated across players");
    assert.equal(result.counts.samples, 9, "three usable participants in each");

    // The paths actually requested — a typo here is the failure this test
    // exists for, and nothing else would catch it.
    assert.ok(asked.some((url) => url.includes("/lol/league/v4/challengerleagues/by-queue/RANKED_SOLO_5x5")));
    assert.ok(asked.some((url) => url.includes("/lol/match/v5/matches/by-puuid/puuid-a/ids?queue=420")));
    assert.ok(asked.some((url) => url.endsWith("/lol/match/v5/matches/M1")));
    assert.ok(asked.some((url) => url.endsWith("/lol/match/v5/matches/M1/timeline")));
    assert.ok(
      asked.some((url) => url.startsWith("https://americas.api.riotgames.com")),
      "match-v5 must go to the regional host",
    );
    assert.ok(
      asked.some((url) => url.startsWith("https://na1.api.riotgames.com")),
      "league-v4 must go to the platform host",
    );

    const written = JSON.parse(readFileSync(join(out, "Ahri", "middle.json"), "utf8"));
    assert.equal(written.champion.key, "Ahri");
    assert.equal(written.role, "middle");
    assert.equal(written.patch, PATCH);
    assert.equal(written.stats.games, 3);
    assert.deepEqual(written.items.core[0].items, [3165, 4645, 3157], "in purchase order");
    assert.deepEqual(written.items.boots[0].items, [3020]);
    // Only Q and E were ever levelled in the fixture, so only those two are
    // claimed. An ability nobody put a point into is not a third priority.
    assert.deepEqual(written.skills.priority, ["Q", "E"]);
    assert.deepEqual(written.skills.order, ["Q", "E", "Q"]);

    // Wukong is filed under its key, not its display name — the app looks it
    // up by key, so a file under Wukong/ would be unreachable.
    assert.ok(existsSync(join(out, "MonkeyKing", "jungle.json")));
    assert.ok(!existsSync(join(out, "Wukong")));

    const output = execFileSync(
      process.execPath,
      ["scripts/ingest/verify-output.mjs", out, "--allow-shrink"],
      { encoding: "utf8" },
    );
    assert.match(output, /3 build files across 3 champions/);
  } finally {
    globalThis.fetch = realFetch;
    rmSync(out, { recursive: true, force: true });
  }
});
