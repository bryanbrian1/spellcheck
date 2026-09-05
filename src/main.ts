/**
 * leaguechecker UI.
 *
 * Vanilla TypeScript, no framework, no runtime dependencies. It talks to Rust
 * two ways — two commands for the search box, two events for champ select —
 * and renders whatever comes back. It deliberately does not know, and must
 * never learn, which provider answered.
 */

type Role = "top" | "jungle" | "middle" | "bottom" | "utility";
type Skill = "Q" | "W" | "E" | "R";

interface BuildStats {
  games?: number;
  winRate?: number;
  pickRate?: number;
  banRate?: number;
}
interface ItemRef { id: number; name?: string }
interface ItemGroup { items: ItemRef[]; stats?: BuildStats; label?: string }
interface ItemPlan {
  starters?: ItemGroup[];
  boots?: ItemGroup[];
  core?: ItemGroup[];
  situational?: ItemGroup[];
}
interface RunePage {
  primaryStyle?: number;
  secondaryStyle?: number;
  primary?: number[];
  secondary?: number[];
  shards?: number[];
  stats?: BuildStats;
  label?: string;
}
interface SummonerSpell { id: number; name?: string }
interface SummonerSet { spells: SummonerSpell[]; stats?: BuildStats }
interface SkillPlan { priority?: Skill[]; order?: Skill[] }
interface SourceInfo {
  providerLabel: string;
  patch?: string;
  region?: string;
  tier?: string;
  updatedAt?: string;
}
interface ChampionRef { key: string; name: string; id?: number }

interface FoundBuild {
  status: "found";
  champion: ChampionRef;
  role: Role;
  source: SourceInfo;
  stats?: BuildStats;
  items: ItemPlan;
  runes?: RunePage[];
  summoners?: SummonerSet[];
  skills?: SkillPlan;
}
interface NoData {
  status: "noData";
  championKey: string;
  championName: string;
  role: Role;
  detail: string;
}
type BuildLookup = FoundBuild | NoData;

/** A champion the client says we locked. `championKey` is the Data Dragon
 *  key (`Ahri`, `MonkeyKing`), which is not always the display name. */
interface LockedChampion {
  championId: number;
  championKey: string;
  assignedPosition: string;
}

/**
 * `lcu:status` — where the League client is, from this app's point of view.
 * `clientOffline` is the ordinary state and arrives on almost every launch;
 * it is a state to render, not an error to report.
 */
type LcuStatus =
  | { event: "clientOffline" }
  | { event: "clientConnected" }
  | { event: "entered" }
  | ({ event: "locked" } & LockedChampion)
  | { event: "left" };

/** `lcu:build` — the build for a locked champion. `lookup` and `error` are
 *  exclusive, and "no data for that pair" lives inside `lookup`. */
interface ChampSelectBuild {
  champion: LockedChampion;
  lookup: BuildLookup | null;
  error: string | null;
}

/**
 * `lcu:suggestions` — what the two rule-based checks made of the composition.
 *
 * Every one of these is a rule: it reasons from what champions and items are,
 * not from a sample, so it wears the amber rail and carries no number. The
 * engine asserts that; this screen must not undo it by rendering a count
 * beside one.
 */
interface Suggestion {
  itemId: number;
  priority: "rush" | "core" | "situational";
  reason: string;
  source: "stat" | "rule";
}

interface ChampSelectSuggestions {
  /** Check one: what the enemy composition forces. */
  threat: Suggestion[];
  /** Check two: what your own team leaves uncovered. */
  gaps: Suggestion[];
  /** Item id to display name. Beside the suggestions, never inside one. */
  itemNames: Record<string, string>;
}

/**
 * `game:state` — the third check, over a game in progress.
 *
 * `standing` is measured and carries its numbers; the two suggestion lists
 * are rules and carry none. That split is why the banner has its own style
 * rather than either rail: it is neither a statistic drawn from thousands of
 * games nor an argument made in words.
 */
interface Standing {
  footing: "behind" | "even" | "ahead";
  opponent: string;
  /** Your spent gold minus theirs. Negative means they are ahead. */
  goldDelta: number;
  levelDelta: number;
  goldInHand: number;
}

interface InGameState {
  champion: string | null;
  /** The Data Dragon key, which is what art is filed under. */
  championKey: string | null;
  level: number;
  gameTime: number;
  standing: Standing | null;
  /** Check one, re-run now the whole enemy team is visible. */
  threat: Suggestion[];
  /** Check three. */
  state: Suggestion[];
  itemNames: Record<string, string>;
}

type InGameUpdate = { event: "noGame" } | ({ event: "playing" } & InGameState);

/** A champion, by the name a player types and the key a provider wants. */
interface Champion {
  /** `MonkeyKing` — what a build is filed under. */
  key: string;
  /** `Wukong` — what anyone would actually type. */
  name: string;
}

/**
 * `data_dragon` — Riot's static data.
 *
 * The champion list for the search box, and the two art tables the UI cannot
 * derive on its own. Items and champion art come from an id and a key we
 * already hold; summoner spells are named rather than numbered, and runes
 * carry their own unversioned path. The page may display Data Dragon images
 * but may not call it, so the backend fetches this.
 */
interface DataDragon {
  /** The full build, `16.17.1`. Not the `16.17` a build file is labelled with. */
  version: string;
  champions: Champion[];
  spells: Record<string, string>;
  perks: Record<string, string>;
}

/** `withGlobalTauri` puts the bridge on window, so we need no npm package. */
declare global {
  interface Window {
    __TAURI__?: {
      core: { invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> };
      event: {
        listen<T>(name: string, handler: (event: { payload: T }) => void): Promise<() => void>;
      };
    };
  }
}

const invoke = <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
  const bridge = window.__TAURI__;
  if (!bridge) return Promise.reject(new Error("not running inside the app window"));
  return bridge.core.invoke<T>(cmd, args);
};

/** Subscribe for the life of the window; nothing here ever unlistens. Outside
 *  the app window (a plain browser) this quietly does nothing. */
const listen = <T,>(name: string, handler: (payload: T) => void): void => {
  const bridge = window.__TAURI__;
  if (!bridge) return;
  void bridge.event.listen<T>(name, (event) => handler(event.payload));
};

const el = <T extends HTMLElement>(id: string): T => {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing #${id}`);
  return node as T;
};

/* ---------- formatting ---------- */

// Rates cross the boundary as fractions. Percent signs are added here and
// nowhere else.
const percent = (rate?: number): string | null =>
  typeof rate === "number" ? `${(rate * 100).toFixed(1)}%` : null;

const count = (n?: number): string | null =>
  typeof n === "number" ? n.toLocaleString("en-US") : null;

/** "53.1% · 18,402 games", or whichever half we actually have. */
const statLine = (stats?: BuildStats): string => {
  if (!stats) return "";
  const wr = percent(stats.winRate);
  const games = count(stats.games);
  const parts: string[] = [];
  if (wr) parts.push(`<b>${wr}</b>`);
  if (games) parts.push(`${games} games`);
  return parts.join(" · ");
};

/**
 * Confidence rail. Sample size drives saturation, so a build off 40 games
 * cannot look as authoritative as one off 18,000. Statistics only — a
 * rule-based block uses the amber rail and never this function.
 */
const railFor = (stats?: BuildStats): string => {
  const games = stats?.games;
  if (typeof games !== "number") return "rail lo";
  if (games >= 1000) return "rail";
  if (games >= 100) return "rail mid";
  return "rail lo";
};

const escape = (raw: string): string =>
  raw.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] as string);

/** Game clock as the scoreboard shows it. */
const clock = (seconds: number): string => {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
};

/* ---------- icons ---------- */

const DDRAGON = "https://ddragon.leagueoflegends.com";

type IconKind = "item" | "champ" | "spell" | "perk";

/** Null until the catalogue arrives, and after a failure. Every tile renders
 *  its text label regardless, so this only ever adds. */
let icons: DataDragon | null = null;

const iconUrl = (kind: IconKind, key: string): string | null => {
  if (!icons) return null;
  switch (kind) {
    case "item":
      return `${DDRAGON}/cdn/${icons.version}/img/item/${key}.png`;
    case "champ":
      return `${DDRAGON}/cdn/${icons.version}/img/champion/${key}.png`;
    case "spell": {
      const file = icons.spells[key];
      return file ? `${DDRAGON}/cdn/${icons.version}/img/spell/${file}` : null;
    }
    case "perk": {
      // Rune art is the one thing Data Dragon serves unversioned.
      const path = icons.perks[key];
      return path ? `${DDRAGON}/cdn/img/${path}` : null;
    }
  }
};

/**
 * Put art into anything that asked for it and does not have it yet.
 *
 * Called after every render, and again when the catalogue arrives, because
 * the two happen in either order — a build can be on screen before Data
 * Dragon has answered. The text label is never removed: it sits underneath,
 * so an image that fails or has not arrived shows the words instead of a
 * gap. Stat shards have no art at all and keep their label permanently.
 */
const paintIcons = (): void => {
  if (!icons) return;
  document.querySelectorAll<HTMLElement>("[data-icon]").forEach((holder) => {
    if (holder.querySelector("img")) return;
    const raw = holder.dataset.icon ?? "";
    const split = raw.indexOf(":");
    if (split < 0) return;
    const src = iconUrl(raw.slice(0, split) as IconKind, raw.slice(split + 1));
    if (!src) return;

    const img = document.createElement("img");
    // Empty alt on purpose: the label is already there behind it, and a
    // broken-image caption would print on top of it.
    img.alt = "";
    img.loading = "lazy";
    img.addEventListener("error", () => img.remove());
    img.src = src;
    holder.prepend(img);
  });
};

/** Champion art in a header, falling back to the initials it used to show. */
const setPortrait = (holder: HTMLElement, key: string | null, fallback: string): void => {
  holder.querySelector("img")?.remove();
  holder.textContent = fallback;
  if (key) holder.dataset.icon = `champ:${key}`;
  else delete holder.dataset.icon;
  paintIcons();
};

/** Providers may or may not resolve names; ids are the guaranteed field. */
const tileLabel = (id: number, name?: string): string =>
  escape(name && name.trim() ? name.trim().slice(0, 9) : `#${id}`);

const tile = (
  id: number,
  name: string | undefined,
  cls = "tile",
  kind: IconKind = "item",
  title?: string,
): string =>
  `<div class="${cls}" data-icon="${kind}:${id}" title="${escape(title ?? name ?? String(id))}">${tileLabel(id, name)}</div>`;

/* ---------- blocks ---------- */

const block = (title: string, meta: string, inner: string, rail = "rail"): string => `
  <div class="block">
    <div class="${rail}"></div>
    <div class="block-in">
      <div class="block-hd"><span class="block-t">${escape(title)}</span><span class="meta">${meta}</span></div>
      ${inner}
    </div>
  </div>`;

/** When the advice wants acting on. Words, not numbers — see the colour law. */
const priorityLabels: Record<Suggestion["priority"], string> = {
  rush: "next back",
  core: "in the core path",
  situational: "if it turns that way",
};

/** Soonest first, which is also the order the engine declares them in. */
const priorityOrder: Suggestion["priority"][] = ["rush", "core", "situational"];

/**
 * One reason, and every item that shares it.
 *
 * A check answering "they all deal physical damage" names both the component
 * you buy now and the item you finish, and printing the same sentence under
 * each of them reads as a stutter. The items are listed together and the
 * argument is made once. They are joined with a comma rather than an arrow:
 * some of these are a build path and some are alternatives, and the renderer
 * cannot tell which.
 */
const sugRow = (group: Suggestion[], names: Record<string, string>): string => {
  const first = group[0];
  if (!first) return "";
  // Every suggestion arrives with a name; the fallback is a word rather than
  // an id because an amber block may not carry a number.
  const named = group.map((s) => names[String(s.itemId)] ?? "Item");
  const tiles = group
    .map((s, i) => tile(s.itemId, named[i], "tile rule"))
    .join("");
  const soonest = priorityOrder.find((p) => group.some((s) => s.priority === p)) ?? "situational";

  return `<div class="sug">
    ${tiles}
    <div class="sug-txt"><b>${escape(named.join(", "))} · ${priorityLabels[soonest]}</b>
      <span class="sug-why">${escape(first.reason)}</span></div>
  </div>`;
};

/**
 * One check's output. Amber rail, and never `railFor` — that function reads a
 * game count, and a rule has none.
 */
const suggestionBlock = (
  title: string,
  suggestions: Suggestion[],
  names: Record<string, string>,
): string => {
  // A suggestion that claims to be a statistic is a bug upstream, and
  // rendering it in amber would be the lie the colour law exists to prevent.
  const rules = suggestions.filter((s) => s.source === "rule");
  if (!rules.length) return "";

  // Group by the argument being made, keeping the order the engine chose.
  const groups: Suggestion[][] = [];
  for (const suggestion of rules) {
    const last = groups[groups.length - 1];
    if (last && last[0]?.reason === suggestion.reason) last.push(suggestion);
    else groups.push([suggestion]);
  }

  return block(title, "reasoning", groups.map((g) => sugRow(g, names)).join(""), "rail rule");
};

/**
 * The standing banner.
 *
 * This one *may* carry a number, and is the only thing in the app besides a
 * statistic that does. It is a measurement of the game being played rather
 * than a sample drawn from many, so it wears neither rail and gets its own
 * style — and "even" is deliberately not the colour that means ahead, because
 * level with your opponent is not good news.
 */
const standingBlock = (standing: Standing): string => {
  const { footing, opponent, goldDelta, levelDelta } = standing;
  const gold =
    footing === "even"
      ? "even"
      : `${goldDelta < 0 ? "\u2212" : "+"}${Math.abs(goldDelta).toLocaleString()}g`;

  const lead = { behind: "Behind", even: "Level with", ahead: "Ahead of" }[footing];
  const levels =
    levelDelta === 0
      ? ""
      : ` \u00b7 ${Math.abs(levelDelta)} level${Math.abs(levelDelta) === 1 ? "" : "s"} ${levelDelta < 0 ? "down" : "up"}`;

  return `<div class="state ${escape(footing)}">
    <span class="state-n">${escape(gold)}</span>
    <span class="state-t"><b>${escape(lead)} ${escape(opponent)}</b>${escape(levels)}</span>
  </div>`;
};

const runeBlock = (page: RunePage): string => {
  const keystone = page.primary?.[0];
  const rest = (page.primary ?? []).slice(1);
  const primary = [
    keystone === undefined ? "" : tile(keystone, undefined, "tile key", "perk"),
    ...rest.map((id) => tile(id, undefined, "tile", "perk")),
  ].join("");
  const secondary = [
    ...(page.secondary ?? []).map((id) => tile(id, undefined, "tile", "perk")),
    // Stat shards are not in runesReforged.json and have no art, so these
    // keep their text label.
    ...(page.shards ?? []).map((id) => tile(id, undefined, "tile sm", "perk")),
  ].join("");
  return block(
    page.label ?? "Runes",
    statLine(page.stats),
    `<div class="row" style="margin-bottom:6px">${primary}</div><div class="row">${secondary}</div>`,
    railFor(page.stats),
  );
};

const summonerBlock = (set: SummonerSet): string =>
  block(
    "Summoners",
    statLine(set.stats),
    `<div class="row">${set.spells.map((s) => tile(s.id, s.name, "tile", "spell")).join("")}</div>`,
    railFor(set.stats),
  );

/** The core path is genuinely a sequence, so it is numbered. */
const coreBlock = (group: ItemGroup): string => {
  const seq = group.items
    .map(
      (item, i) => `<div class="seq-i">
        <span class="seq-n">${i + 1}</span>
        ${tile(item.id, item.name)}
        <span class="seq-lbl">${escape(item.name ?? `#${item.id}`)}</span>
      </div>`,
    )
    .join("");
  return block(
    group.label ?? "Core build",
    statLine(group.stats),
    `<div class="seq">${seq}</div>`,
    railFor(group.stats),
  );
};

/** A tile's tooltip when it is one of several alternatives: its own name and
 *  its own sample, since the header can no longer speak for it. */
const itemTitle = (item: ItemRef, stats?: BuildStats): string => {
  const name = item.name ?? `#${item.id}`;
  const wr = percent(stats?.winRate);
  const games = count(stats?.games);
  const tail = [wr, games && `${games} games`].filter(Boolean).join(" · ");
  return tail ? `${name} · ${tail}` : name;
};

/**
 * A row of items.
 *
 * Two different shapes arrive here and they need opposite treatment.
 * "Starting items" is one group holding several items you buy together.
 * "Boots" and "Situational" are several groups of *one item each* — a menu of
 * alternatives — and rendering only the first turned a menu into a single
 * suggestion, throwing away everything the provider offered.
 */
const itemRowBlock = (title: string, groups: ItemGroup[]): string => {
  const first = groups[0];
  if (!first) return "";

  const single = groups.length === 1;
  const shown = single
    ? first.items.map((item) => ({ item, stats: first.stats }))
    : groups.flatMap((group) => group.items.map((item) => ({ item, stats: group.stats })));

  const row = shown
    .map(({ item, stats }) => tile(item.id, item.name, "tile", "item", itemTitle(item, stats)))
    .join("");

  // One win rate in the header would read as covering every item beside it,
  // which is exactly the kind of borrowed authority the colour law forbids.
  // With alternatives, the header counts them and each tile carries its own.
  const meta = single ? statLine(first.stats) : `${shown.length} options`;
  return block(title, meta, `<div class="row">${row}</div>`, railFor(first.stats));
};

const skillBlock = (skills: SkillPlan): string => {
  const priority = skills.priority ?? [];
  const order = skills.order ?? [];
  if (!priority.length && !order.length) return "";
  const meta = priority.length ? priority.join(" → ") : "";
  const row = order.map((s) => `<div class="tile sm">${s}</div>`).join("");
  return block("Skill order", meta, `<div class="row">${row}</div>`, "rail lo");
};

/* ---------- rendering ---------- */

const results = el<HTMLDivElement>("results");
const importRunes = el<HTMLButtonElement>("import-runes");
const importItems = el<HTMLButtonElement>("import-items");

const emptyHtml = (text: string): string => `<div class="empty"><p>${escape(text)}</p></div>`;

const badHtml = (text: string): string =>
  `<div class="thin bad"><b>Couldn't load that.</b> ${escape(text)}</div>`;

const message = (text: string, bad = false): void => {
  results.innerHTML = bad ? badHtml(text) : emptyHtml(text);
  importRunes.disabled = true;
  importItems.disabled = true;
};

/**
 * The blocks for one found build. Both screens render through this, so a
 * build looked up from champ select and one typed into the search box are
 * the same thing on screen — which they are.
 *
 * Null means the provider answered with a build carrying nothing renderable.
 */
const buildHtml = (lookup: FoundBuild): string | null => {
  const blocks = [
    ...(lookup.runes ?? []).slice(0, 1).map(runeBlock),
    ...(lookup.summoners ?? []).slice(0, 1).map(summonerBlock),
    itemRowBlock("Starting items", lookup.items.starters ?? []),
    ...(lookup.items.core ?? []).slice(0, 1).map(coreBlock),
    itemRowBlock("Boots", lookup.items.boots ?? []),
    itemRowBlock("Situational", lookup.items.situational ?? []),
    lookup.skills ? skillBlock(lookup.skills) : "",
  ].filter(Boolean);

  if (!blocks.length) return null;

  const src = lookup.source;
  const provenance = [src.patch && `patch ${src.patch}`, src.region, src.tier]
    .filter(Boolean)
    .join(" · ");

  return blocks.join("") + (provenance ? `<p class="note">${escape(provenance)}</p>` : "");
};

const render = (lookup: BuildLookup): void => {
  if (lookup.status === "noData") {
    message(`No data for ${lookup.championName} ${lookup.role}. ${lookup.detail}`);
    return;
  }

  const html = buildHtml(lookup);
  if (html === null) {
    message(`${lookup.champion.name} came back with an empty build.`);
    return;
  }

  results.innerHTML = html;
  paintIcons();

  // Import stays disabled: CLAUDE.md requires these to be user-initiated, and
  // the LCU layer only reads — it has no code that could write a rune page.
  importRunes.disabled = true;
  importItems.disabled = true;
};

/* ---------- interaction ---------- */

/* ---------- champion matching ---------- */

/** Filled once Data Dragon answers. Until then the box behaves as it always
 *  did: whatever you typed is sent as-is. */
let champions: Champion[] = [];

/**
 * The name to show for a Data Dragon key.
 *
 * Champ select hands us a key, and twenty-one champions are filed under
 * something no player calls them — `MonkeyKing` for Wukong, `Chogath` for
 * Cho'Gath. Showing the key looks like a bug because it is one. Falls back to
 * the key itself, which is what the screen showed before the roster existed.
 */
const nameFor = (key: string): string =>
  champions.find((champion) => champion.key === key)?.name ?? key;

/** Apostrophes, spaces and full stops are things a player types and a key
 *  never contains — `Kai'Sa` is filed as `Kaisa`, `Dr. Mundo` as `DrMundo`. */
const normalise = (raw: string): string => raw.toLowerCase().replace(/[^a-z0-9]/g, "");

/** First letters of each word, so "mf" finds Miss Fortune. */
const initials = (name: string): string =>
  name
    .split(/[^A-Za-z0-9]+/)
    .filter(Boolean)
    .map((word) => word[0]?.toLowerCase() ?? "")
    .join("");

/**
 * Champions matching what has been typed, best first.
 *
 * Ranked rather than filtered, because "ah" should reach Ahri before Ahri is
 * buried under everything containing those letters. An exact match wins
 * outright: someone who typed a whole name meant it.
 */
const matchChampions = (query: string, limit = 8): Champion[] => {
  const q = normalise(query);
  if (!q) return [];

  const scored: Array<{ champion: Champion; score: number }> = [];
  for (const champion of champions) {
    const name = normalise(champion.name);
    const key = normalise(champion.key);
    let score: number;
    if (name === q || key === q) score = 0;
    else if (name.startsWith(q)) score = 1;
    else if (key.startsWith(q)) score = 2;
    else if (q.length >= 2 && initials(champion.name).startsWith(q)) score = 3;
    else if (name.includes(q) || key.includes(q)) score = 4;
    else continue;
    scored.push({ champion, score });
  }

  scored.sort((a, b) => a.score - b.score || a.champion.name.localeCompare(b.champion.name));
  return scored.slice(0, limit).map((entry) => entry.champion);
};

const champInput = el<HTMLInputElement>("champ");
let role: Role = "middle";
let inFlight = 0;

/** The champion the user actually chose, if they chose one. */
let picked: Champion | null = null;

const lookup = async (): Promise<void> => {
  const typed = champInput.value.trim();
  if (!typed) {
    message("Type a champion and pick a role.");
    return;
  }

  // A provider is asked for the key, and a player types the name. Resolve
  // here rather than sending raw text, which is why "Wukong" used to fail:
  // the build is filed under MonkeyKing. An unrecognised string is still
  // passed through, so an exact key typed by hand keeps working and a real
  // mistake still produces a real error.
  const resolved = picked ?? matchChampions(typed, 1)[0] ?? null;
  const champion = resolved?.key ?? typed;
  const shown = resolved?.name ?? typed;

  const ticket = ++inFlight;
  message(`Looking up ${shown}…`);
  try {
    const result = await invoke<BuildLookup>("fetch_build", { champion, role });
    if (ticket !== inFlight) return; // a newer query already won
    render(result);
  } catch (error) {
    if (ticket !== inFlight) return;
    message(error instanceof Error ? error.message : String(error), true);
  }
};

/* ---------- the suggestion list ---------- */

const suggest = el<HTMLUListElement>("suggest");
let matches: Champion[] = [];
let highlighted = -1;

const closeSuggestions = (): void => {
  matches = [];
  highlighted = -1;
  suggest.hidden = true;
  suggest.innerHTML = "";
  champInput.setAttribute("aria-expanded", "false");
};

const drawSuggestions = (): void => {
  if (matches.length === 0) {
    closeSuggestions();
    return;
  }
  suggest.innerHTML = matches
    .map((champion, index) => {
      // The key is worth showing only where it differs from the name, which
      // is exactly the case a player cannot guess.
      const key =
        champion.key.toLowerCase() === champion.name.toLowerCase().replace(/[^a-z0-9]/g, "")
          ? ""
          : `<span class="sug-key">${escape(champion.key)}</span>`;
      return `<li role="option" data-key="${escape(champion.key)}" aria-selected="${index === highlighted}">
        <span class="sug-portrait" data-icon="champ:${escape(champion.key)}"></span>
        <span>${escape(champion.name)}</span>${key}
      </li>`;
    })
    .join("");
  suggest.hidden = false;
  champInput.setAttribute("aria-expanded", "true");
  paintIcons();
};

const move = (delta: number): void => {
  if (matches.length === 0) return;
  highlighted = (highlighted + delta + matches.length) % matches.length;
  [...suggest.children].forEach((li, index) =>
    li.setAttribute("aria-selected", String(index === highlighted)),
  );
  suggest.children[highlighted]?.scrollIntoView({ block: "nearest" });
};

/** Commit to a champion: the box shows its name, the lookup uses its key. */
const choose = (champion: Champion): void => {
  picked = champion;
  champInput.value = champion.name;
  closeSuggestions();
  window.clearTimeout(debounce);
  void lookup();
};

let debounce = 0;
champInput.addEventListener("input", () => {
  // Anything typed replaces an earlier choice, so Enter takes the best match
  // rather than whatever was picked three keystrokes ago.
  picked = null;
  matches = matchChampions(champInput.value);
  highlighted = matches.length > 0 ? 0 : -1;
  drawSuggestions();

  // One letter matches a dozen champions, so guessing from it would spend a
  // request on whichever happens to sort first. Enter still works at any
  // length: that is the user saying they meant it.
  window.clearTimeout(debounce);
  if (normalise(champInput.value).length >= 2) {
    debounce = window.setTimeout(lookup, 350);
  }
});

champInput.addEventListener("keydown", (event) => {
  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      move(1);
      break;
    case "ArrowUp":
      event.preventDefault();
      move(-1);
      break;
    case "Enter": {
      event.preventDefault();
      const chosen = matches[highlighted] ?? matches[0];
      if (chosen) choose(chosen);
      else {
        window.clearTimeout(debounce);
        void lookup();
      }
      break;
    }
    case "Escape":
      closeSuggestions();
      break;
  }
});

suggest.addEventListener("click", (event) => {
  const item = (event.target as HTMLElement).closest<HTMLLIElement>("li[data-key]");
  if (!item) return;
  const chosen = matches.find((champion) => champion.key === item.dataset.key);
  if (chosen) choose(chosen);
});

// Clicking away puts the list down; it must not sit over the build.
document.addEventListener("click", (event) => {
  if (!(event.target as HTMLElement).closest(".search-wrap")) closeSuggestions();
});

el<HTMLDivElement>("roles").addEventListener("click", (event) => {
  const button = (event.target as HTMLElement).closest<HTMLButtonElement>(".role");
  if (!button) return;
  for (const other of document.querySelectorAll<HTMLButtonElement>(".role")) {
    other.setAttribute("aria-pressed", String(other === button));
  }
  role = (button.dataset.role ?? "middle") as Role;
  void lookup();
});

const screens: Record<string, string> = {
  search: "s-search",
  select: "s-select",
  game: "s-game",
};

/** Show one screen and keep the mode strip agreeing with it, whether the
 *  change came from a click or from the client. */
const showScreen = (mode: string): void => {
  for (const [name, id] of Object.entries(screens)) {
    el(id).classList.toggle("on", name === mode);
  }
  for (const button of document.querySelectorAll<HTMLButtonElement>(".modes button")) {
    button.setAttribute("aria-pressed", String(button.dataset.go === mode));
  }
};

for (const button of document.querySelectorAll<HTMLButtonElement>(".modes button")) {
  button.addEventListener("click", () => showScreen(button.dataset.go ?? "search"));
}

/* ---------- champ select ---------- */

/*
 * Driven entirely by two events from Rust. Nothing here polls, nothing here
 * asks the client anything, and nothing here knows a WebSocket exists — the
 * LCU layer has already reduced all of it to "the client is offline" or "you
 * locked Ahri mid".
 */

const roleLabels: Record<string, string> = {
  top: "Top",
  jungle: "Jungle",
  middle: "Mid",
  bottom: "Bot",
  utility: "Support",
};

const selectPortrait = el<HTMLDivElement>("select-portrait");
const selectName = el<HTMLDivElement>("select-name");
const selectSub = el<HTMLDivElement>("select-sub");
const selectPill = el<HTMLSpanElement>("select-pill");
const selectBody = el<HTMLDivElement>("select-body");

/** The champion the client last said we locked, so a build that arrives after
 *  a fast swap can be recognised as stale and dropped. */
let locked: LockedChampion | null = null;

/** Teal for connected, neutral for not. Amber is never used here: it means
 *  "rule-based suggestion" everywhere else in the app. */
const setPill = (text: string, connected: boolean): void => {
  selectPill.textContent = text;
  selectPill.className = connected ? "pill" : "pill off";
};

/* The champ select body is written by two independent sources: the build,
   which arrives once when you lock in, and the suggestions, which change
   every time one of the other nine players picks. Each holds its own half so
   a late enemy pick does not wipe the build off the screen. */
let buildHalf = "";
let suggestionHalf = "";

const paintSelect = (): void => {
  selectBody.innerHTML = buildHalf + suggestionHalf;
  paintIcons();
};

const selectMessage = (text: string, bad = false): void => {
  buildHalf = bad ? badHtml(text) : emptyHtml(text);
  selectBody.innerHTML = buildHalf + suggestionHalf;
};

/** A new champ select, or none at all. Both halves go. */
const clearSelect = (): void => {
  buildHalf = "";
  suggestionHalf = "";
};

/** Header for a champion we have no build for yet. */
const showLocked = (champion: LockedChampion): void => {
  const name = nameFor(champion.championKey);
  setPortrait(selectPortrait, champion.championKey, name.slice(0, 2).toUpperCase());
  selectName.textContent = name;
  selectSub.textContent =
    roleLabels[champion.assignedPosition] ?? champion.assignedPosition;
};

const showWaiting = (name: string, sub: string): void => {
  setPortrait(selectPortrait, null, "—");
  selectName.textContent = name;
  selectSub.textContent = sub;
};

const onStatus = (status: LcuStatus): void => {
  switch (status.event) {
    // The default state of the machine. Said plainly, not as a failure.
    case "clientOffline":
      locked = null;
      clearSelect();
      showWaiting("Champ select", "Waiting for the League client");
      setPill("Offline", false);
      selectMessage("League isn't running. This screen fills itself when you lock a champion.");
      break;

    case "clientConnected":
      locked = null;
      clearSelect();
      showWaiting("Champ select", "Client is open");
      setPill("Connected", true);
      selectMessage("Waiting for champ select.");
      break;

    case "entered":
      // A fresh champ select. Last game's enemy team must not survive into it.
      clearSelect();
      showWaiting("Champ select", "Pick your champion");
      setPill("In select", true);
      selectMessage("Lock a champion and the build appears here.");
      // You are in champ select now; this is the screen you want.
      showScreen("select");
      break;

    case "locked":
      locked = status;
      showLocked(status);
      setPill("Live", true);
      selectMessage(`Looking up ${nameFor(status.championKey)}…`);
      showScreen("select");
      break;

    // Champ select ended — dodged, or the game is loading. The build stays on
    // screen, because the minute after champ select is exactly when it gets
    // read. Only the pill changes.
    case "left":
      setPill("Connected", true);
      if (locked) {
        const role = roleLabels[locked.assignedPosition] ?? locked.assignedPosition;
        selectSub.textContent = `${role} · champ select ended`;
      }
      break;
  }
};

const onBuild = (payload: ChampSelectBuild): void => {
  // A build for a champion we are no longer locked into lost the race.
  if (locked && payload.champion.championKey !== locked.championKey) return;

  if (payload.error !== null) {
    selectMessage(payload.error, true);
    return;
  }

  const lookup = payload.lookup;
  if (lookup === null) {
    selectMessage(`Nothing came back for ${nameFor(payload.champion.championKey)}.`);
    return;
  }

  if (lookup.status === "noData") {
    // The backend falls back to the key when it has no display name, so
    // prefer the roster: "no data for MonkeyKing" reads as a fault in the app
    // rather than an ordinary answer about Wukong.
    const name = nameFor(lookup.championKey);
    selectName.textContent = name;
    selectMessage(`No data for ${name} ${lookup.role}. ${lookup.detail}`);
    return;
  }

  selectName.textContent = lookup.champion.name;
  const html = buildHtml(lookup);
  buildHalf = html ?? emptyHtml(`${lookup.champion.name} came back with an empty build.`);
  paintSelect();
};

/**
 * The rule-based checks. These arrive on their own schedule — before the
 * build if you locked last, and repeatedly as the rest of the lobby picks —
 * so they replace only their own half of the screen.
 */
const onSuggestions = (payload: ChampSelectSuggestions): void => {
  suggestionHalf =
    suggestionBlock("Against this team", payload.threat, payload.itemNames) +
    suggestionBlock("For your team", payload.gaps, payload.itemNames);
  paintSelect();
};

/* ---------- in game ---------- */

const gamePortrait = el<HTMLDivElement>("game-portrait");
const gameName = el<HTMLDivElement>("game-name");
const gameSub = el<HTMLDivElement>("game-sub");
const gamePill = el<HTMLSpanElement>("game-pill");
const gameBody = el<HTMLDivElement>("game-body");

/* The header moves every time we look; the advice moves a handful of times a
   game. Rewriting the blocks on every reading would redraw the screen every
   thirty seconds for nothing, so the last rendering is kept and compared. */
let lastGameBlocks = "";

const paintGameBody = (html: string): void => {
  if (html === lastGameBlocks) return;
  lastGameBlocks = html;
  gameBody.innerHTML = html;
  paintIcons();
};

const onGameState = (update: InGameUpdate): void => {
  if (update.event === "noGame") {
    setPortrait(gamePortrait, null, "\u2014");
    gameName.textContent = "In game";
    gameSub.textContent = "No game running";
    gamePill.textContent = "No game";
    gamePill.className = "pill off";
    paintGameBody(emptyHtml("This screen fills itself once a game starts."));
    return;
  }

  const champion = update.champion;
  setPortrait(
    gamePortrait,
    update.championKey,
    champion ? champion.slice(0, 2).toUpperCase() : "\u2014",
  );
  gameName.textContent = champion ?? "In game";
  gameSub.textContent = `${clock(update.gameTime)}${update.level ? ` \u00b7 level ${update.level}` : ""}`;
  gamePill.textContent = "Live";
  gamePill.className = "pill";

  const blocks = [
    update.standing ? standingBlock(update.standing) : "",
    suggestionBlock("Because you are behind", update.state, update.itemNames),
    suggestionBlock("Against this team", update.threat, update.itemNames),
  ]
    .filter(Boolean)
    .join("");

  // Spectating, or a mode with no lanes and a champion we have no tags for.
  paintGameBody(
    blocks || emptyHtml("Nothing to say about this game yet."),
  );
};

listen<LcuStatus>("lcu:status", onStatus);
listen<ChampSelectBuild>("lcu:build", onBuild);
listen<ChampSelectSuggestions>("lcu:suggestions", onSuggestions);
listen<InGameUpdate>("game:state", onGameState);

/* Icons are an enhancement, never a requirement: every tile has already drawn
   its text label by the time this resolves, and a failure leaves those in
   place. Fetched once, then applied to whatever is on screen. */
void invoke<DataDragon>("data_dragon")
  .then((catalog) => {
    icons = catalog;
    champions = catalog.champions;
    paintIcons();
  })
  .catch(() => {
    /* no art this session; the labels stand on their own */
  });

// Attribution for whichever source is live. Rendered verbatim, never branched on.
void invoke<string>("source_label")
  .then((label) => {
    const foot = document.querySelector("#s-search .foot");
    if (foot) foot.insertAdjacentHTML("beforeend", `<span class="src">${escape(label)}</span>`);
  })
  .catch(() => {
    /* the footer simply carries no attribution if the bridge is unavailable */
  });

export {};
