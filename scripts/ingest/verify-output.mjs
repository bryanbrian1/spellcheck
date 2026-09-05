#!/usr/bin/env node
//! Guard between the crawler and the commit.
//!
//! `RiotProvider` is deliberately forgiving at runtime: an empty file, a
//! missing file, and a build with no items all resolve to "no data for this
//! pair yet" rather than an error, because an incremental crawl means most
//! pairs genuinely have nothing. That forgiveness is right in the app and
//! wrong in CI — it means a crawl that collapsed halfway (an expiring key, a
//! rate limit, a Riot outage) can write hollow files that commit cleanly and
//! then read as "no data" forever, with nothing anywhere saying it broke.
//!
//! So the shape is checked here, once, while failing is still free.
//!
//! Usage:  node scripts/ingest/verify-output.mjs [data/builds] [--allow-shrink]

import { readdirSync, readFileSync, statSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, basename } from "node:path";

// Must track SCHEMA_VERSION in src-tauri/src/build_data/riot/file.rs. A file
// newer than the app is a hard error there, so it is a hard error here.
const SCHEMA_VERSION = 1;
const ROLES = new Set(["top", "jungle", "middle", "bottom", "utility"]);

const args = process.argv.slice(2);
const allowShrink = args.includes("--allow-shrink");
const root = args.find((arg) => !arg.startsWith("--")) ?? "data/builds";

const inActions = process.env.GITHUB_ACTIONS === "true";
const problems = [];
const note = (path, detail) => problems.push(`${path}: ${detail}`);

/** Mirrors validate_champion_key() so CI rejects what the app would refuse. */
const isChampionKey = (key) =>
  key.length > 0 && key.length <= 32 && /^[A-Za-z0-9]+$/.test(key);

function checkBuildFile(path, champion, role) {
  const raw = readFileSync(path, "utf8");

  // The case this whole script exists for: a truncated write the app would
  // silently read as "nothing here yet".
  if (raw.trim() === "") {
    note(path, "file is empty — the crawler wrote nothing into it");
    return;
  }

  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    note(path, `not valid JSON (${error.message})`);
    return;
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    note(path, "top level is not a JSON object");
    return;
  }

  const version = parsed.schemaVersion;
  if (!Number.isInteger(version) || version < 1) {
    note(path, `schemaVersion must be a positive integer, got ${JSON.stringify(version)}`);
  } else if (version > SCHEMA_VERSION) {
    note(
      path,
      `schemaVersion ${version} is newer than the app reads (${SCHEMA_VERSION}); ` +
        "bump SCHEMA_VERSION in src-tauri/src/build_data/riot/file.rs first",
    );
  }

  // The path is the lookup key, so a file that disagrees with its own path is
  // unreachable: the app would never ask for it under the name it claims.
  const declaredKey = parsed.champion?.key;
  if (declaredKey != null && declaredKey !== champion) {
    note(path, `declares champion "${declaredKey}" but sits in ${champion}/`);
  }
  if (parsed.role != null && parsed.role !== role) {
    note(path, `declares role "${parsed.role}" but is filed as ${role}.json`);
  }

  const core = parsed.items?.core;
  if (!Array.isArray(core) || core.length === 0) {
    note(path, "no core items — the app reads this as 'no build data yet'");
    return;
  }
  if (!core.some((group) => Array.isArray(group?.items) && group.items.length > 0)) {
    note(path, "every core item group is empty");
  }
}

if (!existsSync(root)) {
  problems.push(`${root}/ does not exist — the crawler produced no output at all`);
}

let fileCount = 0;
let championCount = 0;

if (existsSync(root)) {
  for (const champion of readdirSync(root).sort()) {
    const championDir = join(root, champion);
    if (!statSync(championDir).isDirectory()) {
      note(championDir, "unexpected file where a champion directory should be");
      continue;
    }
    if (!isChampionKey(champion)) {
      note(championDir, "not a valid Data Dragon champion key (ASCII alphanumeric, 1-32 chars)");
      continue;
    }

    championCount += 1;
    const files = readdirSync(championDir).filter((name) => name.endsWith(".json"));
    if (files.length === 0) {
      note(championDir, "champion directory holds no role files");
      continue;
    }

    for (const file of files) {
      const role = basename(file, ".json");
      if (!ROLES.has(role)) {
        note(join(championDir, file), `"${role}" is not one of ${[...ROLES].join(", ")}`);
        continue;
      }
      fileCount += 1;
      checkBuildFile(join(championDir, file), champion, role);
    }
  }
}

// A crawl that returns fewer builds than we already have is the signature of a
// run that died partway through. Committing it would delete working data.
let committedCount = 0;
try {
  const tracked = execFileSync("git", ["ls-files", "--", `${root}/*.json`], {
    encoding: "utf8",
    // git writes "fatal: ... is outside repository" here when `root` points
    // somewhere else, which is a normal way to run this script by hand. The
    // throw is already handled below; the stderr line would just look like a
    // failure in the CI log.
    stdio: ["ignore", "pipe", "ignore"],
  }).trim();
  committedCount = tracked === "" ? 0 : tracked.split("\n").length;
} catch {
  // Not a git checkout, or git is unavailable. The shape checks still stand.
}

if (fileCount < committedCount && !allowShrink) {
  problems.push(
    `the crawl produced ${fileCount} build files but ${committedCount} are already committed — ` +
      "a shrinking dataset means the run died partway through. " +
      "Pass --allow-shrink if builds were removed on purpose.",
  );
}

const plural = (count, noun) => `${count} ${noun}${count === 1 ? "" : "s"}`;
const summary = `${plural(fileCount, "build file")} across ${plural(championCount, "champion")} in ${root}/`;

if (problems.length > 0) {
  const title = `Crawl output failed verification (${problems.length} problem${problems.length === 1 ? "" : "s"})`;
  const body = [
    `Found ${summary}.`,
    "",
    ...problems.map((problem) => `  - ${problem}`),
    "",
    "Nothing was committed. The working tree still holds the output so it can",
    "be inspected in the run's logs before the next attempt.",
  ].join("\n");

  if (inActions) console.log(`::error title=${title}::${body.replace(/\n/g, "%0A")}`);
  console.error(`\n${title}\n${"-".repeat(title.length)}\n${body}\n`);
  process.exit(1);
}

console.log(`Verified ${summary}. Safe to commit.`);
