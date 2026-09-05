//! Matches in, build files out.
//!
//! Everything here is pure: it takes the JSON Riot returned and returns
//! plain objects. That is deliberate, because this is where the crawl can be
//! quietly wrong in ways no HTTP error would reveal — a build path assembled
//! in the wrong order, a win rate counted twice, a champion filed under the
//! wrong role. Those are testable, so they are tested, and nothing in this
//! file touches the network or the filesystem.
//!
//! The one thing worth knowing before reading: a finished match does not say
//! what order anything was bought in. Its participant records hold only the
//! final inventory. Build *order* — and skill order — exist only in the match
//! timeline, which is a second request per match and therefore halves how
//! many matches a run can afford. It is bought anyway, because a build path
//! presented in an order nobody verified would be a fabrication, and the
//! order is the product.

const SKILL_LETTERS = { 1: "Q", 2: "W", 3: "E", 4: "R" };
const ROLES = new Set(["top", "jungle", "middle", "bottom", "utility"]);

/** How many finished items count as the core path. */
const CORE_LENGTH = 3;

/** Purchases this early are the opening buy rather than a first back. */
const STARTER_WINDOW_MS = 60_000;

/** How many level-ups the recorded order covers. Beyond about here every
 *  game diverges, and a modal eighteen-level sequence would be one player's
 *  game rather than a pattern. */
const SKILL_ORDER_LENGTH = 5;

/** How many of each kind survive into the file. The rest are a long tail of
 *  one-off games that would bloat every file for no reader. */
const KEEP = { starters: 2, boots: 3, core: 3, situational: 6, runes: 2, summoners: 2 };

export function roleOf(participant) {
  const raw = String(participant?.teamPosition ?? "").toLowerCase();
  return ROLES.has(raw) ? raw : null;
}

/**
 * Item purchases per participant, in order, with undos applied.
 *
 * An undo is not rare — misclicking in the shop and taking it back is
 * ordinary — and leaving them in would put items in build paths that were
 * never actually owned.
 */
export function purchasesByParticipant(timeline) {
  const purchases = new Map();
  const push = (id) => {
    if (!purchases.has(id)) purchases.set(id, []);
    return purchases.get(id);
  };

  for (const frame of timeline?.info?.frames ?? []) {
    for (const event of frame?.events ?? []) {
      const participant = event?.participantId;
      if (!Number.isInteger(participant) || participant < 1) continue;

      if (event.type === "ITEM_PURCHASED" && Number.isInteger(event.itemId)) {
        push(participant).push({ itemId: event.itemId, timestamp: event.timestamp ?? 0 });
      } else if (event.type === "ITEM_UNDO") {
        // `beforeId` is what the undo removed. Drop the most recent purchase
        // of it rather than the first, so a repeated buy-and-undo unwinds in
        // the order it happened.
        const list = push(participant);
        for (let i = list.length - 1; i >= 0; i -= 1) {
          if (list[i].itemId === event.beforeId) {
            list.splice(i, 1);
            break;
          }
        }
      }
    }
  }
  return purchases;
}

/** Skill level-ups per participant, as slot numbers in the order taken. */
export function skillsByParticipant(timeline) {
  const skills = new Map();
  for (const frame of timeline?.info?.frames ?? []) {
    for (const event of frame?.events ?? []) {
      if (event?.type !== "SKILL_LEVEL_UP") continue;
      const participant = event.participantId;
      if (!Number.isInteger(participant) || !SKILL_LETTERS[event.skillSlot]) continue;
      if (!skills.has(participant)) skills.set(participant, []);
      skills.get(participant).push(event.skillSlot);
    }
  }
  return skills;
}

/**
 * Which basic ability was maxed first, second, third.
 *
 * Ranked by the level-up at which each reached five points. An ability that
 * never got there is ranked behind the ones that did, by how many points it
 * ended with — a game that ended at level nine still says which way the
 * player was going.
 */
export function maxOrder(slots) {
  const points = { 1: 0, 2: 0, 3: 0 };
  const maxedAt = {};
  const firstTaken = {};

  slots.forEach((slot, index) => {
    if (!(slot in points)) return;
    if (!(slot in firstTaken)) firstTaken[slot] = index;
    points[slot] += 1;
    if (points[slot] === 5 && !(slot in maxedAt)) maxedAt[slot] = index;
  });

  const taken = Object.keys(points)
    .map(Number)
    .filter((slot) => points[slot] > 0);
  if (taken.length === 0) return [];

  taken.sort((a, b) => {
    const aMaxed = a in maxedAt;
    const bMaxed = b in maxedAt;
    if (aMaxed !== bMaxed) return aMaxed ? -1 : 1;
    if (aMaxed && bMaxed) return maxedAt[a] - maxedAt[b];
    if (points[a] !== points[b]) return points[b] - points[a];
    return firstTaken[a] - firstTaken[b];
  });

  return taken.map((slot) => SKILL_LETTERS[slot]);
}

function runesOf(participant) {
  const styles = participant?.perks?.styles ?? [];
  const primary = styles.find((style) => style?.description === "primaryStyle");
  const secondary = styles.find((style) => style?.description === "subStyle");
  const stat = participant?.perks?.statPerks ?? {};
  const perks = (style) =>
    (style?.selections ?? []).map((selection) => selection?.perk).filter(Number.isInteger);

  if (!primary?.style) return null;
  return {
    primaryStyle: primary.style,
    primary: perks(primary),
    secondaryStyle: secondary?.style ?? null,
    secondary: perks(secondary),
    // Offence, flex, defence — the order the client shows them in, not the
    // order the API happens to list them.
    shards: [stat.offense, stat.flex, stat.defense].filter(Number.isInteger),
  };
}

/**
 * One usable observation per participant, or nothing.
 *
 * A match contributes ten samples at most and often fewer: an unknown
 * champion, a queue that assigned no position, or a build with no finished
 * item in it all drop out here rather than downstream.
 */
export function samplesFromMatch(match, timeline, catalog) {
  const info = match?.info;
  if (!info || !Array.isArray(info.participants)) return [];

  const purchases = purchasesByParticipant(timeline);
  const skills = skillsByParticipant(timeline);
  const samples = [];

  for (const participant of info.participants) {
    const role = roleOf(participant);
    const champion = catalog.champion(participant?.championId);
    if (!role || !champion) continue;

    const bought = purchases.get(participant.participantId) ?? [];
    const starters = [];
    const core = [];
    const situational = [];
    let boots = null;

    for (const { itemId, timestamp } of bought) {
      const item = catalog.item(itemId);
      if (!item || item.trinket) continue;

      if (timestamp < STARTER_WINDOW_MS) starters.push(itemId);
      if (item.boots && boots === null) boots = itemId;
      if (item.finished) {
        if (core.length < CORE_LENGTH) core.push(itemId);
        else if (!situational.includes(itemId)) situational.push(itemId);
      }
    }

    // A build path is the point of the file. Without one there is nothing
    // here worth a sample, and the output guard would reject the file anyway.
    if (core.length === 0) continue;

    const slots = skills.get(participant.participantId) ?? [];
    const summoners = [participant.summoner1Id, participant.summoner2Id]
      .filter(Number.isInteger)
      .sort((a, b) => a - b);

    samples.push({
      championId: champion.id,
      championKey: champion.key,
      championName: champion.name,
      role,
      win: participant.win === true,
      // Sorted: a start is a set, not a sequence, and two players who bought
      // the same two items in a different click order started the same way.
      starters: [...new Set(starters)].sort((a, b) => a - b),
      boots,
      core,
      situational,
      runes: runesOf(participant),
      summoners,
      skillOrder: slots.slice(0, SKILL_ORDER_LENGTH).map((slot) => SKILL_LETTERS[slot]),
      skillPriority: maxOrder(slots),
    });
  }

  return samples;
}

/* ---------- tallying ---------- */

const bump = (map, key, seed, win) => {
  let entry = map.get(key);
  if (!entry) {
    entry = { ...seed, games: 0, wins: 0 };
    map.set(key, entry);
  }
  entry.games += 1;
  if (win) entry.wins += 1;
};

export function emptyTally() {
  return new Map();
}

export function pairKey(championKey, role) {
  return `${championKey}/${role}`;
}

export function addSample(tally, sample) {
  const key = pairKey(sample.championKey, sample.role);
  let entry = tally.get(key);
  if (!entry) {
    entry = {
      championKey: sample.championKey,
      championName: sample.championName,
      championId: sample.championId,
      role: sample.role,
      games: 0,
      wins: 0,
      starters: new Map(),
      boots: new Map(),
      core: new Map(),
      situational: new Map(),
      runes: new Map(),
      summoners: new Map(),
      skillPriority: new Map(),
      skillOrder: new Map(),
    };
    tally.set(key, entry);
  }

  entry.games += 1;
  if (sample.win) entry.wins += 1;

  if (sample.starters.length > 0) {
    bump(entry.starters, sample.starters.join("-"), { items: sample.starters }, sample.win);
  }
  if (sample.boots !== null) {
    bump(entry.boots, String(sample.boots), { items: [sample.boots] }, sample.win);
  }
  bump(entry.core, sample.core.join("-"), { items: sample.core }, sample.win);
  for (const item of sample.situational) {
    bump(entry.situational, String(item), { items: [item] }, sample.win);
  }
  if (sample.runes) {
    const runeKey = [
      sample.runes.primaryStyle,
      ...sample.runes.primary,
      sample.runes.secondaryStyle,
      ...sample.runes.secondary,
      ...sample.runes.shards,
    ].join("-");
    bump(entry.runes, runeKey, { page: sample.runes }, sample.win);
  }
  if (sample.summoners.length === 2) {
    bump(entry.summoners, sample.summoners.join("-"), { spells: sample.summoners }, sample.win);
  }
  if (sample.skillPriority.length > 0) {
    bump(entry.skillPriority, sample.skillPriority.join(""), { priority: sample.skillPriority }, sample.win);
  }
  if (sample.skillOrder.length > 0) {
    bump(entry.skillOrder, sample.skillOrder.join(""), { order: sample.skillOrder }, sample.win);
  }
  return entry;
}

/** Most played first. Ties break on wins and then on the key itself, so two
 *  runs over the same games produce byte-identical files. */
function rank(map, keep) {
  return [...map.entries()]
    .sort(([keyA, a], [keyB, b]) => b.games - a.games || b.wins - a.wins || keyA.localeCompare(keyB))
    .slice(0, keep)
    .map(([, entry]) => entry);
}

const winRate = (wins, games) => (games > 0 ? Number((wins / games).toFixed(4)) : 0);

const itemGroup = (entry) => ({
  items: entry.items,
  games: entry.games,
  winRate: winRate(entry.wins, entry.games),
});

/**
 * One tallied pair as the file the app reads.
 *
 * Field names and nesting here are a contract with
 * `src-tauri/src/build_data/riot/file.rs` — note that a group's stats are
 * flattened into the group rather than nested under it.
 */
export function tallyToFile(entry, { patch, region, tier, updatedAt }) {
  const most = (map) => rank(map, 1)[0] ?? null;
  const priority = most(entry.skillPriority);
  const order = most(entry.skillOrder);

  return {
    schemaVersion: 1,
    champion: { key: entry.championKey, name: entry.championName, id: entry.championId },
    role: entry.role,
    patch,
    region,
    tier,
    updatedAt,
    stats: { games: entry.games, winRate: winRate(entry.wins, entry.games) },
    items: {
      starters: rank(entry.starters, KEEP.starters).map(itemGroup),
      boots: rank(entry.boots, KEEP.boots).map(itemGroup),
      core: rank(entry.core, KEEP.core).map(itemGroup),
      situational: rank(entry.situational, KEEP.situational).map(itemGroup),
    },
    runes: rank(entry.runes, KEEP.runes).map((rune) => ({
      primaryStyle: rune.page.primaryStyle,
      secondaryStyle: rune.page.secondaryStyle,
      primary: rune.page.primary,
      secondary: rune.page.secondary,
      shards: rune.page.shards,
      games: rune.games,
      winRate: winRate(rune.wins, rune.games),
    })),
    summoners: rank(entry.summoners, KEEP.summoners).map((set) => ({
      spells: set.spells,
      games: set.games,
      winRate: winRate(set.wins, set.games),
    })),
    skills: {
      priority: priority?.priority ?? [],
      order: order?.order ?? [],
    },
  };
}

export { KEEP, CORE_LENGTH, STARTER_WINDOW_MS, SKILL_ORDER_LENGTH };
