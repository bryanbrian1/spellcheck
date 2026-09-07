#!/usr/bin/env node
//! Preflight: is RIOT_API_KEY usable *right now*?
//!
//! Riot's development keys expire 24 hours after they are issued, so a
//! scheduled crawl will pass once and then fail every run afterwards. That
//! failure is worth catching before the crawler starts: a key that dies
//! mid-crawl leaves a half-written tree, and a key that was already dead
//! produces an empty one. Neither should reach a commit.
//!
//! Runs standalone too:  RIOT_API_KEY=RGAPI-... node scripts/ingest/check-key.mjs

// A cheap authenticated endpoint. We only care about the status code — the
// body is irrelevant, so this is the smallest request that proves a key works.
const PROBE = "https://na1.api.riotgames.com/lol/status/v4/platform-data";
const TIMEOUT_MS = 10_000;
const DEV_PORTAL = "https://developer.riotgames.com/";

const inActions = process.env.GITHUB_ACTIONS === "true";

/** Loud in CI (a red annotation on the run), plain on a terminal. */
function fail(title, lines) {
  const body = lines.join("\n");
  if (inActions) {
    // Annotations are single-line, so the newlines are escaped for the
    // summary and the readable copy goes to the log underneath.
    console.log(`::error title=${title}::${body.replace(/\n/g, "%0A")}`);
  }
  console.error(`\n${title}\n${"-".repeat(title.length)}\n${body}\n`);
  process.exit(1);
}

const key = (process.env.RIOT_API_KEY ?? "").trim();

if (!key) {
  fail("RIOT_API_KEY is not set", [
    "The ingest job needs a Riot API key and found none.",
    "",
    "Set it as a repository secret (the value never appears in a log):",
    "  gh secret set RIOT_API_KEY --repo bryanbrian1/spellcheck",
    "",
    "Then confirm it exists with:  gh secret list --repo bryanbrian1/spellcheck",
  ]);
}

// Shape check first — it costs no request and catches a truncated paste or a
// value that picked up surrounding quotes. Only a warning: if Riot ever
// changes the prefix, a working key must not be rejected by our own guess.
if (!/^RGAPI-[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(key)) {
  const note =
    "RIOT_API_KEY does not look like a Riot key (expected RGAPI- followed by a UUID). " +
    `Got ${key.length} characters starting "${key.slice(0, 6)}". Probing anyway.`;
  if (inActions) console.log(`::warning::${note}`);
  else console.warn(`warning: ${note}`);
}

let response;
try {
  response = await fetch(PROBE, {
    headers: { "X-Riot-Token": key },
    signal: AbortSignal.timeout(TIMEOUT_MS),
  });
} catch (error) {
  // A network failure says nothing about the key, and saying "expired" here
  // would send someone to regenerate a key that was fine.
  fail("Could not reach the Riot API", [
    `${PROBE} did not answer within ${TIMEOUT_MS / 1000}s.`,
    `Cause: ${error?.message ?? error}`,
    "",
    "This is a connectivity problem, not a key problem. The key was not checked.",
  ]);
}

switch (response.status) {
  case 200:
    console.log("RIOT_API_KEY accepted by the Riot API. Proceeding with the crawl.");
    break;

  case 401:
    fail("Riot rejected the key outright (401)", [
      "The key was sent but Riot did not recognise it at all.",
      "That usually means the secret holds something other than a key —",
      "an empty value, a stray newline, or quotes captured with the paste.",
      "",
      "Re-set it:  gh secret set RIOT_API_KEY --repo bryanbrian1/spellcheck",
    ]);
    break;

  case 403:
    // Riot returns a bare 403 for both an expired key and a revoked one and
    // does not distinguish them in the body, so this names the likely cause
    // without asserting it.
    fail("Riot refused the key (403) — it has almost certainly expired", [
      "Development keys expire 24 hours after they are issued, and a 403 on a",
      "key that worked yesterday is what that expiry looks like. A revoked key",
      "and a key from the wrong account also land here.",
      "",
      `Regenerate at ${DEV_PORTAL} and re-set the secret:`,
      "  gh secret set RIOT_API_KEY --repo bryanbrian1/spellcheck",
      "",
      "If this keeps happening on a schedule, a 24-hour key is the wrong tool:",
      "apply for a Personal or Production key, which do not expire daily.",
      "",
      "No crawl ran, so nothing was written and nothing was committed.",
    ]);
    break;

  case 429:
    fail("Rate limited before the crawl even started (429)", [
      `Riot asked us to wait ${response.headers.get("retry-after") ?? "an unspecified time"}s.`,
      "The key is valid; something else is already spending its budget.",
      "Re-run this workflow once the window clears.",
    ]);
    break;

  default:
    if (response.status >= 500) {
      fail(`The Riot API is unhealthy (${response.status})`, [
        "Riot returned a server error, so the key could not be verified.",
        "This is their side, not ours. Re-run later.",
      ]);
    }
    fail(`Unexpected response from the Riot API (${response.status})`, [
      `${PROBE} answered ${response.status} ${response.statusText}.`,
      "The key was neither confirmed nor clearly rejected, so the crawl is",
      "being stopped rather than run against an unknown state.",
    ]);
}
