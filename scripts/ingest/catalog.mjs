//! Data Dragon: the patch, the champion names, and what an item *is*.
//!
//! The match API answers in numeric ids. Turning those into the build files
//! the app reads needs two things it never sends: the champion key a file is
//! filed under, and whether an item is a component, a pair of boots, or the
//! finished thing a build path is made of. Both come from Data Dragon, which
//! is Riot's own static data and needs no key.
//!
//! Nothing from here is committed. The build files hold ids only, exactly so
//! they stay small and never go stale on a rename.

const DDRAGON = "https://ddragon.leagueoflegends.com";

/**
 * `15.17.1` is a build; `15.17` is the patch people talk about and the one a
 * build is or is not current for. Comparing at build level would throw away
 * every game the moment a hotfix shipped.
 */
export function patchOf(version) {
  const [major, minor] = String(version).split(".");
  return major && minor ? `${major}.${minor}` : String(version);
}

/**
 * Whether a match was played on the patch we are crawling for.
 *
 * `gameVersion` looks like `15.17.681.9271`, so only the first two parts are
 * comparable with a Data Dragon version.
 */
export function isOnPatch(gameVersion, patch) {
  return patchOf(gameVersion) === patch;
}

/**
 * The gold floor for "a finished item".
 *
 * Nothing builds out of a finished item, but that alone also describes
 * Doran's Ring, every trinket, and a control ward. The floor is what
 * separates the things a build path is made of from the things you open with,
 * and it sits below the cheapest completed item rather than near the average.
 */
const FINISHED_ITEM_FLOOR = 2000;

function classifyItem(id, item) {
  const cost = item.gold?.total ?? 0;
  const into = Array.isArray(item.into) ? item.into : [];
  const tags = Array.isArray(item.tags) ? item.tags : [];
  const purchasable = item.gold?.purchasable !== false;
  const consumable = item.consumed === true || tags.includes("Consumable");
  const boots = tags.includes("Boots");

  return {
    id,
    name: item.name ?? String(id),
    cost,
    purchasable,
    consumable,
    boots,
    // Free means a trinket, which every player has and nobody chooses.
    trinket: purchasable && cost === 0 && !consumable,
    component: into.length > 0,
    finished:
      purchasable && !consumable && !boots && into.length === 0 && cost >= FINISHED_ITEM_FLOOR,
  };
}

/**
 * Load the static data the crawl needs.
 *
 * `fetchJson` is injected so this can be exercised without the network.
 */
export async function loadCatalog(fetchJson = defaultFetchJson) {
  const versions = await fetchJson(`${DDRAGON}/api/versions.json`);
  const version = Array.isArray(versions) ? versions[0] : null;
  if (!version) throw new Error("Data Dragon returned no versions");

  const [championData, itemData] = await Promise.all([
    fetchJson(`${DDRAGON}/cdn/${version}/data/en_US/champion.json`),
    fetchJson(`${DDRAGON}/cdn/${version}/data/en_US/item.json`),
  ]);

  const champions = new Map();
  for (const [key, champion] of Object.entries(championData.data ?? {})) {
    const numericId = Number(champion.key);
    if (!Number.isInteger(numericId)) continue;
    champions.set(numericId, { key, name: champion.name ?? key, id: numericId });
  }

  const items = new Map();
  for (const [rawId, item] of Object.entries(itemData.data ?? {})) {
    const id = Number(rawId);
    // Above three hundred thousand are the other maps' variants of the same
    // items. Summoner's Rift never sends them, and letting them in would
    // split a build path in two.
    if (!Number.isInteger(id) || id >= 300_000) continue;
    items.set(id, classifyItem(id, item));
  }

  if (champions.size === 0 || items.size === 0) {
    throw new Error("Data Dragon returned no champions or no items");
  }

  return {
    version,
    patch: patchOf(version),
    champion: (id) => champions.get(Number(id)) ?? null,
    item: (id) => items.get(Number(id)) ?? null,
    championCount: champions.size,
    itemCount: items.size,
  };
}

async function defaultFetchJson(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(20_000) });
  if (!response.ok) throw new Error(`${response.status} from ${url}`);
  return response.json();
}
