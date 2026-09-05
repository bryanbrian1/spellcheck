//! Reading, merging and writing `data/builds/{Champion}/{role}.json`.
//!
//! A single run cannot gather a useful sample. A development key allows fifty
//! requests a minute, a match costs two of them, and there are around eight
//! hundred and fifty champion-role pairs to spread the result across — one
//! run is a few dozen games per pair, which is below the threshold at which
//! the app is willing to call a number a statistic.
//!
//! So a run does not replace the dataset, it adds to it. Yesterday's counts
//! are read back, today's are added, and the file is rewritten. That is the
//! whole reason the scheduled crawl is worth running daily rather than once.
//!
//! With one exception, which is the important part: **a new patch throws the
//! old sample away.** Items change, champions change, and a build path
//! averaged across a patch boundary describes a game nobody is playing. When
//! the patch in the file does not match the patch being crawled, the file is
//! replaced rather than merged.

import { existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

import { KEEP } from "./aggregate.mjs";

/** Files hold a rate; merging needs counts. The round-trip is lossy in the
 *  last decimal place and exact enough for a tally of whole games. */
const winsOf = (entry) => Math.round((entry.games ?? 0) * (entry.winRate ?? 0));

const winRate = (wins, games) => (games > 0 ? Number((wins / games).toFixed(4)) : 0);

export function buildPath(root, championKey, role) {
  return join(root, championKey, `${role}.json`);
}

export function readBuild(root, championKey, role) {
  const path = buildPath(root, championKey, role);
  if (!existsSync(path)) return null;
  try {
    const parsed = JSON.parse(readFileSync(path, "utf8"));
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : null;
  } catch {
    // A file we cannot read is one we are about to overwrite with a good one.
    // Failing the run over it would strand every other pair.
    return null;
  }
}

export function writeBuild(root, championKey, role, file) {
  mkdirSync(join(root, championKey), { recursive: true });
  writeFileSync(buildPath(root, championKey, role), `${JSON.stringify(file, null, 2)}\n`);
}

/**
 * Combine two lists of counted things, keeping the most played.
 *
 * `identity` decides what counts as the same entry — the same item path, the
 * same rune page. Entries that fall outside `keep` are dropped, which is what
 * makes the files stay a fixed size however long the crawl accumulates.
 */
export function mergeCounted(existing, fresh, identity, keep) {
  const merged = new Map();

  for (const list of [existing ?? [], fresh ?? []]) {
    for (const entry of list) {
      if (!entry || typeof entry !== "object") continue;
      const key = identity(entry);
      const seen = merged.get(key);
      if (seen) {
        seen.games += entry.games ?? 0;
        seen.wins += winsOf(entry);
      } else {
        merged.set(key, { ...entry, games: entry.games ?? 0, wins: winsOf(entry) });
      }
    }
  }

  return [...merged.entries()]
    .sort(([keyA, a], [keyB, b]) => b.games - a.games || b.wins - a.wins || keyA.localeCompare(keyB))
    .slice(0, keep)
    .map(([, entry]) => {
      const { wins, ...rest } = entry;
      return { ...rest, winRate: winRate(wins, entry.games) };
    });
}

const itemIdentity = (group) => (group.items ?? []).join("-");
const runeIdentity = (page) =>
  [page.primaryStyle, ...(page.primary ?? []), page.secondaryStyle, ...(page.secondary ?? []), ...(page.shards ?? [])].join("-");
const summonerIdentity = (set) => (set.spells ?? []).join("-");

/**
 * Yesterday's file plus today's crawl.
 *
 * Returns `fresh` unchanged when there is nothing to merge with, or when the
 * patch moved — see the module docs for why a patch boundary is a reset
 * rather than a join.
 */
export function mergeBuildFiles(existing, fresh) {
  if (!existing) return fresh;
  if (existing.patch !== fresh.patch) return fresh;
  if (existing.schemaVersion !== fresh.schemaVersion) return fresh;

  const games = (existing.stats?.games ?? 0) + (fresh.stats?.games ?? 0);
  const wins = winsOf(existing.stats ?? {}) + winsOf(fresh.stats ?? {});

  return {
    ...fresh,
    stats: { games, winRate: winRate(wins, games) },
    items: {
      starters: mergeCounted(existing.items?.starters, fresh.items?.starters, itemIdentity, KEEP.starters),
      boots: mergeCounted(existing.items?.boots, fresh.items?.boots, itemIdentity, KEEP.boots),
      core: mergeCounted(existing.items?.core, fresh.items?.core, itemIdentity, KEEP.core),
      situational: mergeCounted(existing.items?.situational, fresh.items?.situational, itemIdentity, KEEP.situational),
    },
    runes: mergeCounted(existing.runes, fresh.runes, runeIdentity, KEEP.runes),
    summoners: mergeCounted(existing.summoners, fresh.summoners, summonerIdentity, KEEP.summoners),
    // Skills are the one thing the file format carries without a sample count
    // behind it, so there is no honest way to weight two answers against each
    // other. The fresh crawl wins: it is drawn from more recent games, and a
    // champion's max order is the most stable thing in a build.
    skills: fresh.skills?.priority?.length || fresh.skills?.order?.length ? fresh.skills : existing.skills,
  };
}

/** Every champion-role pair already committed, so a run can report how much
 *  of the dataset it actually touched. */
export function existingPairs(root) {
  const pairs = new Set();
  if (!existsSync(root)) return pairs;
  for (const champion of readdirSync(root)) {
    const dir = join(root, champion);
    if (!statSync(dir).isDirectory()) continue;
    for (const file of readdirSync(dir)) {
      if (file.endsWith(".json")) pairs.add(`${champion}/${file.slice(0, -5)}`);
    }
  }
  return pairs;
}
