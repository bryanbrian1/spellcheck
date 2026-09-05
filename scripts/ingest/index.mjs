#!/usr/bin/env node
//! The crawl.
//!
//! Turns Riot's match API into `data/builds/{Champion}/{role}.json`, which is
//! the distributable half of this app — the only build data we may ship,
//! because it is ours rather than borrowed.
//!
//! The shape of the job is decided entirely by the rate limit. A development
//! key allows a hundred requests every two minutes, so fifty a minute is the
//! real ceiling however generous the per-second burst looks. A match costs
//! two requests (the match, then its timeline, which is the only place build
//! order and skill order exist), so an hour of crawling is on the order of a
//! thousand matches. Ten participants each, spread over eight hundred and
//! fifty champion-role pairs, is a few dozen games per pair.
//!
//! That is why this accumulates rather than replaces: see store.mjs. One run
//! is not a dataset. Thirty are.
//!
//!   RIOT_API_KEY=RGAPI-... node scripts/ingest/index.mjs
//!
//! Everything else has a default. `INGEST_MINUTES` is the one worth knowing:
//! the run stops on the clock, because a job killed by the workflow timeout
//! commits nothing, and stopping early with most of the data is a good run.

import { appendFileSync } from "node:fs";

import { OutOfBudget, RiotApi, RiotApiError } from "./api.mjs";
import { isOnPatch, loadCatalog } from "./catalog.mjs";
import { addSample, emptyTally, samplesFromMatch, tallyToFile } from "./aggregate.mjs";
import { existingPairs, mergeBuildFiles, readBuild, writeBuild } from "./store.mjs";

const env = (name, fallback) => process.env[name] ?? fallback;
const number = (name, fallback) => {
  const raw = process.env[name];
  if (raw === undefined || raw === "") return fallback;
  const value = Number(raw);
  if (!Number.isFinite(value) || value <= 0) throw new Error(`${name} must be a positive number`);
  return value;
};

const RANKED_SOLO = 420;
const APEX_LEAGUES = ["challenger", "grandmaster"];

const config = {
  key: env("RIOT_API_KEY", ""),
  platform: env("RIOT_PLATFORM", "na1"),
  limits: env("RIOT_RATE_LIMITS", "20:1,100:120"),
  out: env("INGEST_OUT", "data/builds"),
  minutes: number("INGEST_MINUTES", 45),
  budget: number("INGEST_BUDGET", Infinity),
  seedPlayers: number("INGEST_SEED_PLAYERS", 150),
  matchesPerPlayer: number("INGEST_MATCHES_PER_PLAYER", 15),
  // A brand new pair needs enough games to be worth a file at all. Pairs that
  // already have a file are always updated, however few games this run saw —
  // that is the point of accumulating.
  minGames: number("INGEST_MIN_GAMES", 5),
};

const started = Date.now();
const log = (...parts) => console.log(new Date().toISOString().slice(11, 19), ...parts);

/** Deterministic shuffle, so a run samples across the pool rather than
 *  working through one player's history, and two runs with the same seed
 *  behave the same when something needs reproducing. */
function shuffle(list, seed = 1) {
  const out = [...list];
  let state = seed >>> 0 || 1;
  for (let i = out.length - 1; i > 0; i -= 1) {
    state = (state * 1664525 + 1013904223) >>> 0;
    const j = state % (i + 1);
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

/** Apex ladder players, as puuids. */
async function seedPlayers(api) {
  const puuids = [];
  const needingLookup = [];

  for (const league of APEX_LEAGUES) {
    if (api.exhausted || puuids.length >= config.seedPlayers) break;
    const body = await api.platformGet(
      `/lol/league/v4/${league}leagues/by-queue/RANKED_SOLO_5x5`,
    );
    const entries = body?.entries ?? [];
    log(`${league}: ${entries.length} entries`);

    for (const entry of entries) {
      if (entry?.puuid) puuids.push(entry.puuid);
      else if (entry?.summonerId) needingLookup.push(entry.summonerId);
    }
  }

  if (puuids.length === 0 && needingLookup.length > 0) {
    // Older league payloads identify players by summoner id only, and each
    // one costs a request to resolve. Capped hard: this is a fallback, not a
    // phase, and it must never eat the budget the matches need.
    const cap = Math.min(needingLookup.length, config.seedPlayers);
    log(`league entries carry no puuid; resolving ${cap} of them the long way`);
    for (const summonerId of needingLookup.slice(0, cap)) {
      if (api.exhausted) break;
      const summoner = await api.platformGet(`/lol/summoner/v4/summoners/${summonerId}`);
      if (summoner?.puuid) puuids.push(summoner.puuid);
    }
  }

  return shuffle(puuids).slice(0, config.seedPlayers);
}

/** Recent ranked solo match ids from those players, deduplicated. */
async function collectMatchIds(api, puuids) {
  const ids = new Set();
  for (const puuid of puuids) {
    if (api.exhausted) break;
    const query = `queue=${RANKED_SOLO}&type=ranked&start=0&count=${config.matchesPerPlayer}`;
    const found = await api.regionGet(`/lol/match/v5/matches/by-puuid/${puuid}/ids?${query}`);
    for (const id of found ?? []) ids.add(id);
  }
  // Apex players play each other constantly, so the same match arrives from
  // several of them. Shuffling stops the crawl from working through one
  // player's week before it reaches anyone else's.
  return shuffle([...ids], 7);
}

/** Seconds, whichever unit Riot used. Older matches report milliseconds. */
const durationSeconds = (info) =>
  info.gameEndTimestamp !== undefined || info.gameDuration < 10_000
    ? info.gameDuration
    : Math.round(info.gameDuration / 1000);

async function crawl() {
  if (!config.key) {
    console.error("RIOT_API_KEY is not set. Nothing to crawl with.");
    process.exit(1);
  }

  const deadline = started + config.minutes * 60_000;
  const api = new RiotApi({ ...config, deadline, log });

  log(`platform ${api.platform} (${api.region}), limits ${config.limits}`);
  log(`stopping at ${new Date(deadline).toISOString().slice(11, 19)} or ${config.budget} requests`);

  const catalog = await loadCatalog();
  log(`patch ${catalog.patch} — ${catalog.championCount} champions, ${catalog.itemCount} items`);

  const counts = { matches: 0, offPatch: 0, tooShort: 0, missing: 0, samples: 0 };
  const tally = emptyTally();
  let stopped = null;

  try {
    const puuids = await seedPlayers(api);
    log(`${puuids.length} seed players`);
    if (puuids.length === 0) throw new Error("no seed players — the ladder endpoints returned nothing usable");

    const matchIds = await collectMatchIds(api, puuids);
    log(`${matchIds.length} distinct matches to work through`);

    for (const matchId of matchIds) {
      if (api.exhausted) break;

      const match = await api.regionGet(`/lol/match/v5/matches/${matchId}`);
      if (!match?.info) {
        counts.missing += 1;
        continue;
      }
      // Filter before buying the timeline. Early in a patch most of the
      // ladder's recent history is off-patch, and paying two requests to find
      // that out would halve the run for nothing.
      if (!isOnPatch(match.info.gameVersion, catalog.patch)) {
        counts.offPatch += 1;
        continue;
      }
      if (durationSeconds(match.info) < 600) {
        counts.tooShort += 1;
        continue;
      }

      const timeline = await api.regionGet(`/lol/match/v5/matches/${matchId}/timeline`);
      if (!timeline?.info) {
        counts.missing += 1;
        continue;
      }

      counts.matches += 1;
      for (const sample of samplesFromMatch(match, timeline, catalog)) {
        addSample(tally, sample);
        counts.samples += 1;
      }

      if (counts.matches % 100 === 0) {
        log(`${counts.matches} matches, ${counts.samples} samples, ${api.spent} requests`);
      }
    }
  } catch (error) {
    if (error instanceof OutOfBudget) {
      stopped = error.message;
    } else if (error instanceof RiotApiError && (error.status === 401 || error.status === 403)) {
      // The key died mid-run. Everything gathered so far is still good, so it
      // gets written — but this exits non-zero, because a key that expires
      // daily is an operational problem and must not pass quietly.
      stopped = `the key stopped working (${error.status})`;
      writeAll(tally, catalog, counts);
      report(api, counts, stopped, true);
      process.exit(1);
    } else {
      throw error;
    }
  }

  writeAll(tally, catalog, counts);
  report(api, counts, stopped ?? "finished the match list", false);
}

function writeAll(tally, catalog, counts) {
  const before = existingPairs(config.out);
  const updatedAt = new Date().toISOString();
  const written = { created: 0, updated: 0, skipped: 0, repatched: 0 };

  for (const entry of tally.values()) {
    const fresh = tallyToFile(entry, {
      patch: catalog.patch,
      region: config.platform,
      tier: APEX_LEAGUES.join("+"),
      updatedAt,
    });

    const existing = readBuild(config.out, entry.championKey, entry.role);
    const merged = mergeBuildFiles(existing, fresh);

    // The output guard rejects a file with no core path, and it is right to:
    // the app reads one as "no data for this pair". Skipping is the same
    // outcome without the failed run.
    if (!merged.items?.core?.length) {
      written.skipped += 1;
      continue;
    }
    const isNew = !before.has(`${entry.championKey}/${entry.role}`);
    if (isNew && merged.stats.games < config.minGames) {
      written.skipped += 1;
      continue;
    }

    if (existing && existing.patch !== fresh.patch) written.repatched += 1;
    writeBuild(config.out, entry.championKey, entry.role, merged);
    if (isNew) written.created += 1;
    else written.updated += 1;
  }

  counts.written = written;
}

function report(api, counts, why, failed) {
  const minutes = ((Date.now() - started) / 60_000).toFixed(1);
  const w = counts.written ?? { created: 0, updated: 0, skipped: 0, repatched: 0 };

  const lines = [
    `Stopped because: ${why}`,
    `${api.spent} requests in ${minutes} min (${(api.throttledMs / 1000).toFixed(0)}s waiting on the rate limit, ${api.retries} retries)`,
    `${counts.matches} matches used, ${counts.samples} samples`,
    `skipped: ${counts.offPatch} off-patch, ${counts.tooShort} too short, ${counts.missing} unavailable`,
    `wrote: ${w.created} new pairs, ${w.updated} updated, ${w.skipped} held back${w.repatched ? `, ${w.repatched} reset for a new patch` : ""}`,
  ];

  console.log(`\n${lines.join("\n")}\n`);

  if (process.env.GITHUB_STEP_SUMMARY) {
    const heading = failed ? "### Crawl failed" : "### Crawl";
    const body = `${heading}\n\n${lines.map((line) => `- ${line}`).join("\n")}\n`;
    try {
      appendFileSync(process.env.GITHUB_STEP_SUMMARY, body);
    } catch {
      /* the summary is a nicety; never fail a run over it */
    }
  }
}

crawl().catch((error) => {
  console.error(`\nThe crawl failed: ${error.message}\n`);
  process.exit(1);
});
