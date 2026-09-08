/**
 * spellcheck UI.
 *
 * Vanilla TypeScript, no framework, no runtime dependencies. It talks to Rust
 * two ways — commands for the search box, events for the live screen — and
 * renders whatever comes back. It deliberately does not know, and must never
 * learn, which provider answered, or which of the two routes into the live
 * screen asked for a build.
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

type LaneAdvantage = "ours" | "theirs" | "even";

/** Present only on a build a source really filtered to one opponent. Its
 *  absence is the signal that this is the general build — see `render`. */
interface MatchupInfo {
  opponent: ChampionRef;
  tip?: string;
  laneAdvantage?: LaneAdvantage;
  playStyle?: string;
}

interface FoundBuild {
  status: "found";
  champion: ChampionRef;
  role: Role;
  source: SourceInfo;
  stats?: BuildStats;
  matchup?: MatchupInfo;
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
  | { event: "unreadable"; reason: string }
  | { event: "left" };

/**
 * `live:build` — the build for the champion we are on.
 *
 * Sent when the client says we locked in, and sent when a game turns out to
 * be running that we never saw the champ select for. The payload is identical
 * either way and carries no hint of which route found it, because there is
 * nothing this screen would do differently. `lookup` and `error` are
 * exclusive, and "no data for that pair" lives inside `lookup`.
 */
interface LiveBuild {
  /** The Data Dragon key, matched against the champion on screen. */
  championKey: string;
  /** The lane we were given. Empty when League named none. */
  position: string;
  /**
   * True when the lane in `lookup` is the champion's most-played one rather
   * than one League assigned — Practice Tool, customs and ARAM name none.
   * The screen has to say so: a lane we chose and a lane you were given are
   * different claims, and only one of them is a fact about your game.
   */
  inferredRole: boolean;
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
  /** Our lane, when the mode assigns one — the one thing champ select and
   *  the game both name, and so the reason either can ask for a build. */
  role: Role | null;
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
  /** Perk id to display name, stat shards included. The label under the art. */
  perkNames: Record<string, string>;
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
    const raw = holder.dataset.icon ?? "";
    const split = raw.indexOf(":");
    if (split < 0) return;
    const kind = raw.slice(0, split) as IconKind;
    const key = raw.slice(split + 1);

    // A tile rendered before the catalogue arrived is still showing "#8112".
    // Correct it now — the label is what a viewer reads when art is slow,
    // blocked, or absent, and for a stat shard it used to be all there was.
    //
    // Written through the label element rather than the holder: assigning to
    // holder.textContent would delete an <img> already prepended below, and on
    // a second pass over a tile whose id never resolved it silently did.
    const label = holder.querySelector<HTMLElement>(".tile-l");
    if (kind === "perk" && label?.textContent?.startsWith("#")) {
      const name = perkName(Number(key));
      if (name) {
        label.textContent = tileLabelText(name);
        holder.title = name;
      }
    }

    if (holder.querySelector("img")) return;
    const src = iconUrl(kind, key);
    if (!src) return;

    const img = document.createElement("img");
    // Empty alt on purpose: the label is already there behind it, and a
    // broken-image caption would print on top of it.
    img.alt = "";
    img.loading = "lazy";
    img.addEventListener("error", () => img.remove());
    // The label stands in for art. Once art has actually arrived it has
    // nothing left to stand in for, and rune art is a transparent symbol
    // rather than an opaque square, so leaving it there prints the words
    // through the gaps. On `load` rather than eagerly: art that never
    // arrives, or fails, must still leave the words in place.
    img.addEventListener("load", () => holder.classList.add("has-art"));
    img.src = src;
    holder.prepend(img);
  });
};

/** Champion art in a header, falling back to the initials it used to show.
 *
 *  The live header repaints on every reading of a game in progress, which is
 *  twice a minute for the whole match. Tearing the image down and putting an
 *  identical one back would make the portrait blink each time, so an
 *  unchanged champion is left alone. */
const setPortrait = (holder: HTMLElement, key: string | null, fallback: string): void => {
  const wanted = key ? `champ:${key}` : "";
  if (holder.dataset.painted === wanted) return;
  holder.dataset.painted = wanted;

  holder.querySelector("img")?.remove();
  holder.classList.remove("has-art");
  holder.textContent = fallback;
  if (key) holder.dataset.icon = `champ:${key}`;
  else delete holder.dataset.icon;
  paintIcons();
};

/**
 * A rune, style or stat shard's display name, once the catalogue has arrived.
 *
 * `undefined` before it does, which is ordinary rather than a problem: the
 * tile draws its id placeholder and `paintIcons` fills the real label in when
 * the catalogue lands, the same way it fills in the art.
 */
const perkName = (id: number): string | undefined => icons?.perkNames?.[String(id)];

/**
 * How much of a name a tile can show.
 *
 * A 24px shard tile cannot render "Adaptive Force" at any honest size, and
 * pretending otherwise by shrinking the type is what produced the 9px
 * captions the redesign already removed. The label is a fallback for when art
 * is missing; the full name always lives in the tile's `title`.
 */
const tileLabelText = (name: string): string => name.trim().slice(0, 9);

/** Providers may or may not resolve names; ids are the guaranteed field. */
const tileLabel = (id: number, name?: string): string =>
  escape(name && name.trim() ? tileLabelText(name) : `#${id}`);

const tile = (
  id: number,
  name: string | undefined,
  cls = "tile",
  kind: IconKind = "item",
  title?: string,
): string =>
  `<div class="${cls}" data-icon="${kind}:${id}" title="${escape(title ?? name ?? String(id))}"><span class="tile-l">${tileLabel(id, name)}</span></div>`;

/* ---------- blocks ---------- */

const block = (title: string, meta: string, inner: string, rail = "rail"): string => `
  <div class="block">
    <div class="${rail}"></div>
    <div class="block-in">
      <div class="block-hd"><span class="block-t">${escape(title)}</span><span class="meta">${meta}</span></div>
      ${inner}
    </div>
  </div>`;

/**
 * What the recommendation engine currently argues for, and who it argued it
 * about.
 *
 * `championKey` is not bookkeeping — it is what stops a category error. The
 * engine reasons about *your* champion against *this* enemy team: which resist
 * answers their damage is a claim about your damage type as much as theirs. So
 * the advice may only annotate a build for the same champion it was computed
 * for. Looking up somebody else mid-game must show that champion's statistics
 * unmarked rather than this champion's reasoning wearing their name.
 */
interface Advice {
  championKey: string | null;
  reasons: Map<number, Suggestion>;
}

const NO_ADVICE: Advice = { championKey: null, reasons: new Map() };

/** Index suggestions by the item they name, keeping the first argument made
 *  for each — the engine emits in priority order, so the first is the most
 *  urgent thing it has to say about that item. */
const adviceFrom = (championKey: string | null, ...lists: (Suggestion[] | undefined)[]): Advice => {
  const reasons = new Map<number, Suggestion>();
  for (const list of lists) {
    for (const suggestion of list ?? []) {
      // Rules only. A "stat" suggestion wearing the amber flag would be the
      // lie the colour law exists to prevent.
      if (suggestion.source !== "rule") continue;
      if (!reasons.has(suggestion.itemId)) reasons.set(suggestion.itemId, suggestion);
    }
  }
  return { championKey, reasons };
};

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

/** One row of a rune page: the tree it belongs to, then its tiles. */
const runeRow = (tree: string | undefined, inner: string): string =>
  inner
    ? `<div class="rune-row">${tree ? `<span class="rune-tree">${escape(tree)}</span>` : ""}<div class="row">${inner}</div></div>`
    : "";

/**
 * The rune page, in the three parts it actually has.
 *
 * Primary tree, secondary tree, stat shards. The secondary runes and the
 * shards used to share a row, so a page read as four icons followed by five
 * unrelated ones, and which tree to open for the second pair was stated
 * nowhere on the screen at all.
 *
 * Both tree names come from Data Dragon by id, which is why the secondary can
 * be named even though the provider only ever tells us the primary one. The
 * shards keep their tiles: Data Dragon does serve their art, so a tile here is
 * a promise the catalogue can keep.
 */
const runeBlock = (page: RunePage): string => {
  const keystone = page.primary?.[0];
  const rest = (page.primary ?? []).slice(1);
  const primary = [
    keystone === undefined ? "" : tile(keystone, perkName(keystone), "tile key", "perk"),
    ...rest.map((id) => tile(id, perkName(id), "tile", "perk")),
  ].join("");
  const secondary = (page.secondary ?? [])
    .map((id) => tile(id, perkName(id), "tile", "perk"))
    .join("");
  // Passing a name matters more here than for the runes above: until it did,
  // these three drew as "#5005" in an unlabelled box and read as empty slots.
  const shards = (page.shards ?? [])
    .map((id) => tile(id, perkName(id), "tile sm", "perk"))
    .join("");

  const treeName = (id?: number): string | undefined =>
    id === undefined ? undefined : perkName(id);

  return block(
    page.label ?? "Runes",
    statLine(page.stats),
    runeRow(treeName(page.primaryStyle), primary) +
      runeRow(treeName(page.secondaryStyle), secondary) +
      runeRow(shards ? "Shards" : undefined, shards),
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
 * A row of items bought together.
 *
 * One group holding several items — "Starting items" is the only shape that
 * fits this, and it is genuinely one purchase: a ring and two potions is a
 * single decision, so one win rate over the lot of them is honest.
 *
 * A *menu* of alternatives is a different shape and gets
 * [`alternativesBlock`] instead.
 */
const itemRowBlock = (title: string, groups: ItemGroup[]): string => {
  const first = groups[0];
  if (!first) return "";

  const row = first.items
    .map((item) => tile(item.id, item.name, "tile", "item", itemTitle(item, first.stats)))
    .join("");

  return block(title, statLine(first.stats), `<div class="row">${row}</div>`, railFor(first.stats));
};

/**
 * A menu of alternatives: several groups of one item each.
 *
 * "Boots" and "Situational" are lists of things you pick *between*, and they
 * used to render as anonymous 32px squares under a header that said "6
 * options" and nothing else. Six unlabelled squares are not six times as
 * useful as one recommendation; they are about as useful as none, because the
 * one question a player has here — which of these, and when — went unanswered.
 *
 * So each option gets its own column: what it is, and how it actually
 * performs. Two separate things are being said at once, and the colour law
 * governs how they may sit together.
 *
 * The numbers are **statistics** and carry themselves. Each option shows its
 * own win rate and its own sample, because the header cannot speak for a menu
 * — and they are sorted by how often the item is really built, so a group
 * seen in three games no longer sits beside one seen in forty as though the
 * app were neutral between them.
 *
 * The flag is a **rule**. When the recommendation engine has independently
 * argued for one of these items in this game, that option is marked amber and
 * captioned with the engine's own priority wording, with its reasoning in the
 * tooltip. It never becomes a number and never edits the statistics beside
 * it: a win rate says how the item performs across thousands of games, the
 * amber says this particular enemy team is why you would reach for it, and
 * conflating those two is the exact failure the colour law exists to prevent.
 */
const alternativesBlock = (title: string, groups: ItemGroup[], advice: Advice): string => {
  if (!groups.length) return "";

  const options = groups
    .flatMap((group) => group.items.map((item) => ({ item, stats: group.stats })))
    // Most-built first. The provider's order is its own business and has no
    // stated meaning, so leaving it alone would be presenting an arbitrary
    // sequence as if it ranked something.
    .sort((a, b) => (b.stats?.games ?? 0) - (a.stats?.games ?? 0));

  if (!options.length) return "";

  const columns = options
    .map(({ item, stats }) => {
      const flagged = advice.reasons.get(item.id);
      const label = escape(item.name ?? `#${item.id}`);
      const stat = statLine(stats);
      // The engine's own words for when it wants the item, never new ones.
      const why = flagged
        ? `<span class="seq-why">${escape(priorityLabels[flagged.priority])}</span>`
        : "";
      return `<div class="seq-i">
        ${tile(item.id, item.name, flagged ? "tile rule" : "tile", "item", flagged ? flagged.reason : itemTitle(item, stats))}
        <span class="seq-lbl">${label}</span>
        ${stat ? `<span class="seq-stat">${stat}</span>` : ""}
        ${why}
      </div>`;
    })
    .join("");

  // With a menu, the header counts it rather than borrowing one option's win
  // rate to stand for all of them — that borrowed authority is the whole
  // reason each column carries its own numbers. With exactly one option there
  // is nothing to be ambiguous about, so the header speaks for it as it
  // always did.
  const meta = options.length === 1 ? statLine(options[0]?.stats) : `${options.length} options`;
  return block(title, meta, `<div class="seq">${columns}</div>`, railFor(options[0]?.stats));
};

const skillBlock = (skills: SkillPlan): string => {
  const priority = skills.priority ?? [];
  const order = skills.order ?? [];
  if (!priority.length && !order.length) return "";
  const meta = priority.length ? priority.join(" → ") : "";
  const row = order.map((s) => `<div class="tile sm">${s}</div>`).join("");
  return block("Skill order", meta, `<div class="row">${row}</div>`, "rail lo");
};

/**
 * The matchup banner.
 *
 * Drawn only from `matchup`, which a source sets only when the build really
 * is filtered to this opponent. It carries no sample of its own — the
 * build's own stats block already reports the matchup's games and win rate —
 * so like the standing banner it wears neither rail: the advantage and the
 * play style are the source's reading of a lane, and the tip is its prose.
 * Neither is a statistic of ours and neither is one of our rules.
 */
const matchupBlock = (matchup: MatchupInfo): string => {
  const advantage: Record<LaneAdvantage, string> = {
    ours: "You have the lane",
    theirs: `${matchup.opponent.name} has the lane`,
    even: "Even lane",
  };

  const lead = matchup.laneAdvantage ? advantage[matchup.laneAdvantage] : "";
  const style = matchup.playStyle ? `play it ${matchup.playStyle}` : "";
  const line = [lead, style].filter(Boolean).join(" \u00b7 ");
  const tip = matchup.tip ? `<p class="vs-tip">${escape(matchup.tip)}</p>` : "";

  return `<div class="vs-hd ${escape(matchup.laneAdvantage ?? "even")}">
    <span class="vs-portrait" data-icon="champ:${escape(matchup.opponent.key)}"></span>
    <span class="vs-txt">
      <b>vs ${escape(matchup.opponent.name)}</b>
      ${line ? `<span class="vs-sub">${escape(line)}</span>` : ""}
    </span>
  </div>${tip}`;
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
const buildHtml = (lookup: FoundBuild, advice: Advice = NO_ADVICE): string | null => {
  // Only this champion's own reasoning may mark this champion's items.
  const mine =
    advice.championKey && advice.championKey === lookup.champion.key ? advice : NO_ADVICE;

  const blocks = [
    ...(lookup.runes ?? []).slice(0, 1).map(runeBlock),
    ...(lookup.summoners ?? []).slice(0, 1).map(summonerBlock),
    itemRowBlock("Starting items", lookup.items.starters ?? []),
    ...(lookup.items.core ?? []).slice(0, 1).map(coreBlock),
    alternativesBlock("Boots", lookup.items.boots ?? [], mine),
    alternativesBlock("Situational", lookup.items.situational ?? [], mine),
    lookup.skills ? skillBlock(lookup.skills) : "",
  ].filter(Boolean);

  if (!blocks.length) return null;

  // Prepended after the emptiness check, never before it: a banner saying who
  // the build is against is not itself a build, and must not be what makes an
  // empty answer look like a full one.
  if (lookup.matchup) blocks.unshift(matchupBlock(lookup.matchup));

  const src = lookup.source;
  const provenance = [src.patch && `patch ${src.patch}`, src.region, src.tier]
    .filter(Boolean)
    .join(" · ");

  return blocks.join("") + (provenance ? `<p class="note">${escape(provenance)}</p>` : "");
};

/**
 * `askedOpponent` is the opponent the caller asked about, if any, and exists
 * for one case: the question was a matchup and the answer is not one. A
 * source with no matchup data answers with the ordinary build and leaves
 * `matchup` unset, and the screen has to say so — silently drawing the
 * general build under the name of an opponent would be a lie the user has no
 * way to catch.
 */
const render = (lookup: BuildLookup, askedOpponent?: string): void => {
  if (lookup.status === "noData") {
    message(`No data for ${lookup.championName} ${lookup.role}. ${lookup.detail}`);
    return;
  }

  const html = buildHtml(lookup);
  if (html === null) {
    message(`${lookup.champion.name} came back with an empty build.`);
    return;
  }

  const unanswered =
    askedOpponent && !lookup.matchup
      ? `<div class="thin">This source has no <b>${escape(askedOpponent)}</b> data. \
Showing the general ${escape(lookup.champion.name)} ${escape(lookup.role)} build.</div>`
      : "";

  results.innerHTML = unanswered + html;
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
const vsInput = el<HTMLInputElement>("vs");

/**
 * Fill the opponent box's datalist, once the roster has arrived.
 *
 * Display names only — the browser matches on what is in the list, and the
 * point of the list is that a player picks a name they recognise. A typed key
 * still works: `lookup` resolves whatever is in the box through the same
 * matcher the champion box uses.
 */
const fillRoster = (): void => {
  const roster = document.getElementById("roster");
  if (!roster) return;
  roster.innerHTML = champions
    .map((champion) => `<option value="${escape(champion.name)}"></option>`)
    .join("");
};

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

  // The same resolution for the opponent, and for the same reason: the box
  // holds a name and a provider wants a key. An empty box asks the ordinary
  // question, which is the common case.
  const typedOpponent = vsInput.value.trim();
  const foundOpponent = typedOpponent ? matchChampions(typedOpponent, 1)[0] ?? null : null;
  const opponent = typedOpponent ? foundOpponent?.key ?? typedOpponent : "";
  const opponentShown = foundOpponent?.name ?? typedOpponent;

  const ticket = ++inFlight;
  message(opponent ? `Looking up ${shown} vs ${opponentShown}…` : `Looking up ${shown}…`);
  try {
    const result = await invoke<BuildLookup>("fetch_build", { champion, role, opponent });
    if (ticket !== inFlight) return; // a newer query already won
    render(result, opponent ? opponentShown : undefined);
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

let vsDebounce = 0;
vsInput.addEventListener("input", () => {
  // Only once the champion box has something to build for — an opponent on
  // its own is not a question anybody can answer.
  window.clearTimeout(vsDebounce);
  if (!champInput.value.trim()) return;
  // Clearing the box is a real change and must re-run: it is how you get back
  // from the matchup build to the general one.
  const typed = normalise(vsInput.value);
  if (typed.length === 0 || typed.length >= 2) {
    vsDebounce = window.setTimeout(lookup, 350);
  }
});

vsInput.addEventListener("keydown", (event: KeyboardEvent) => {
  if (event.key !== "Enter") return;
  event.preventDefault();
  window.clearTimeout(vsDebounce);
  void lookup();
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
  live: "s-live",
};

/** Show one screen and keep the mode strip agreeing with it, whether the
 *  change came from a click or from League. */
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

/* ---------- the live screen ---------- */

/*
 * Champ select and the game are one screen, because they are one run of
 * League. Four event streams write to it and none of them coordinates:
 *
 *   lcu:status       where the client is — offline, in select, locked in
 *   lcu:suggestions  checks one and two, over the champ select lobby
 *   game:state       check three, and check one again, over the live game
 *   live:build       the build, from whichever of the two routes found it
 *
 * Each owns its own slots below and never writes another's. That is what
 * makes the handover work: when the game starts it adds blocks at the top of
 * a screen that is already filled in, rather than clearing everything and
 * beginning again somewhere else. Nothing polls, nothing here asks League
 * anything, and nothing here knows a WebSocket or an HTTP poll exists.
 */

const roleLabels: Record<string, string> = {
  top: "Top",
  jungle: "Jungle",
  middle: "Mid",
  bottom: "Bot",
  utility: "Support",
};

/**
 * The lane, as a person would say it.
 *
 * Practice Tool, customs and ARAM assign nobody one, and champ select now
 * reports those locks rather than swallowing them — so this has to render a
 * blank. A blank line under the champion's name reads as a bug, so it says
 * what is actually true instead.
 */
const laneLabel = (position: string): string =>
  roleLabels[position] ?? (position || "No lane in this mode");

const livePortrait = el<HTMLDivElement>("live-portrait");
const liveName = el<HTMLDivElement>("live-name");
const liveSub = el<HTMLDivElement>("live-sub");
const livePill = el<HTMLSpanElement>("live-pill");
const liveBody = el<HTMLDivElement>("live-body");

/**
 * The champion the client last said we locked.
 *
 * Held past the end of champ select on purpose. The client stops talking
 * about champ select the moment it closes, but the champion it named is the
 * one about to be played, and it is what a late build is matched against.
 */
let locked: LockedChampion | null = null;

/** The latest reading of a game in progress, or null when none is running.
 *  Its presence is what puts this screen into its second half. */
let playing: InGameState | null = null;

/**
 * The last reading of a game that has since ended.
 *
 * The build deliberately survives the final whistle — the minutes after a
 * game are exactly when somebody reads what they should have built. But a
 * build for the game you *played* and a build for the game you are *playing*
 * are the same pixels making two different claims, and without this the
 * screen quietly reverted to looking live. Holding the finished reading lets
 * the header say which one it is showing.
 *
 * Cleared by [`clearLive`], so a fresh champ select never opens against it.
 */
let lastGame: InGameState | null = null;

/** The build, whichever route found it, or what the lookup came back with
 *  instead of one. */
let buildSlot = "";

/** Check one and check two, as champ select saw them. */
let threatSlot = "";
let gapsSlot = "";

/** What the client is doing. Shown only while there is nothing better on
 *  screen, which once a game is running there always is. */
let noticeSlot = "";

/** The header, while no game is running. A game overrides all of it. */
let selectSub = "Waiting for the League client";
let selectPill: [text: string, connected: boolean] = ["Offline", false];

/**
 * Check three's heading.
 *
 * The advice underneath differs by footing — components and defence when
 * behind, spikes when ahead — so the heading has to as well. A fixed "because
 * you are behind" was wrong every game that was going well.
 */
/**
 * Said above a build whose lane nobody assigned.
 *
 * Practice Tool, customs and ARAM name no lane, so the app asks the source
 * which lane the champion is actually played in and shows that. This is the
 * sentence that keeps the answer honest — without it the header's lane reads
 * as the one you were given, which in those modes there was never any.
 */
const inferredNote = (lookup: FoundBuild): string =>
  `<div class="thin"><b>No lane in this mode.</b> Showing ${escape(lookup.champion.name)} ${escape(
    roleLabels[lookup.role] ?? lookup.role,
  )}, the lane it is played in most.</div>`;

const stateTitle = (standing: Standing | null): string => {
  switch (standing?.footing) {
    case "behind":
      return "Because you are behind";
    case "ahead":
      return "Because you are ahead";
    default:
      return "How the game is going";
  }
};

/** Teal for connected, neutral for not. Amber is never used here: it means
 *  "rule-based suggestion" everywhere else in the app. */
const paintHeader = (): void => {
  // A game outranks the client. Once one is running the champion, the lane
  // and the clock all come from the thing actually being played, and the
  // client has nothing left to say that this screen would rather show.
  // A finished game still names the champion and lane this screen is about,
  // so it outranks champ select here for the same reason a running one does.
  const game = playing ?? lastGame;
  const key = game?.championKey ?? locked?.championKey ?? null;
  const name = game?.champion ?? (locked ? nameFor(locked.championKey) : null);
  const [pill, connected] = playing
    ? ["In game", true]
    : // Not an error state, so not the red "off" pill — the game simply
      // finished. It reads as past tense rather than as something wrong.
      lastGame
      ? ["Game over", false]
      : selectPill;

  setPortrait(livePortrait, key, name ? name.slice(0, 2).toUpperCase() : "\u2014");
  liveName.textContent = name ?? "Live";
  liveSub.textContent = playing
    ? [
        playing.role ? roleLabels[playing.role] : null,
        clock(playing.gameTime),
        playing.level ? `level ${playing.level}` : null,
      ]
        .filter(Boolean)
        .join(" \u00b7 ")
    : lastGame
      ? // Same shape champ select ending uses, for the same reason: the thing
        // on screen is still worth reading, and it is over.
        [lastGame.role ? roleLabels[lastGame.role] : null, "game ended"]
          .filter(Boolean)
          .join(" \u00b7 ")
      : selectSub;
  livePill.textContent = pill;
  livePill.className = connected ? "pill" : "pill off";
};

/* A game reports itself every half-minute for the length of the match and
   almost nothing in it moves between readings. The last rendering is kept and
   compared so the screen — including the build somebody is reading — is not
   rebuilt twice a minute for a clock that lives in the header. */
let lastBody = "";

const paintBody = (): void => {
  const blocks: string[] = [];

  // The standing and the advice beside it describe a game that is running —
  // "Zed is five thousand gold up on you" is a fact with a tense. They hang
  // off `playing` alone and drop out the moment it ends.
  if (playing) {
    if (playing.standing) blocks.push(standingBlock(playing.standing));
    blocks.push(suggestionBlock(stateTitle(playing.standing), playing.state, playing.itemNames));
  }

  // Check one is different: it describes the enemy composition, which does not
  // change when the game ends. The game's reading is strictly the better one —
  // champ select is often looking at five hidden seats, a game never is — so
  // it replaces champ select's answer rather than sitting beside it
  // disagreeing, and it goes on doing so afterwards. Reverting to the champ
  // select reading at the final whistle would change the answer on screen
  // while nobody was looking, for the worse.
  const enemies = playing ?? lastGame;
  if (enemies) {
    blocks.push(suggestionBlock("Against this team", enemies.threat, enemies.itemNames));
  } else {
    blocks.push(threatSlot);
  }

  // Check two is champ select's alone and survives into the game untouched:
  // your own team's shape was settled when the last ally locked in, and no
  // item anyone buys afterwards changes it.
  blocks.push(gapsSlot);

  // Last, because everything above it is a reason to deviate from it, and a
  // reason reads better before the plan it modifies. It also means the game
  // starting inserts blocks at the top instead of shuffling the build.
  blocks.push(buildSlot);

  const html = blocks.filter(Boolean).join("");
  const shown =
    html || (playing ? emptyHtml("Nothing to say about this game yet.") : noticeSlot);

  if (shown === lastBody) return;
  lastBody = shown;
  liveBody.innerHTML = shown;
  paintIcons();
};

const paintLive = (): void => {
  paintHeader();
  paintBody();
};

/** A new run of League. Everything either half had to say about the last one
 *  goes, so a fresh champ select cannot open against the previous game. */
const clearLive = (): void => {
  buildSlot = "";
  threatSlot = "";
  gapsSlot = "";
  lastGame = null;
  liveBuild = null;
  advice = NO_ADVICE;
};

const onStatus = (status: LcuStatus): void => {
  switch (status.event) {
    // The default state of the machine. Said plainly, not as a failure.
    case "clientOffline":
      locked = null;
      clearLive();
      selectSub = "Waiting for the League client";
      selectPill = ["Offline", false];
      noticeSlot = emptyHtml(
        "League isn't running. This screen fills itself when you lock a champion.",
      );
      break;

    case "clientConnected":
      locked = null;
      clearLive();
      selectSub = "Client is open";
      selectPill = ["Connected", true];
      noticeSlot = emptyHtml("Waiting for champ select.");
      break;

    // Champ select is running and the client's payload no longer parses the
    // way this app expects. Shown rather than swallowed: it is otherwise
    // identical on screen to "you have not locked in", which would leave
    // somebody waiting through a whole champ select for a build that was
    // never coming.
    case "unreadable":
      locked = null;
      clearLive();
      selectSub = "Can't read champ select";
      selectPill = ["Confused", false];
      noticeSlot = emptyHtml(
        "League changed something this app reads, so it can't tell what you picked. " +
          "Search for your champion above and the build still works.",
      );
      showScreen("live");
      break;

    case "entered":
      // A fresh champ select. Last game's champion and enemy team must not
      // survive into it.
      locked = null;
      clearLive();
      selectSub = "Pick your champion";
      selectPill = ["In select", true];
      noticeSlot = emptyHtml("Lock a champion and the build appears here.");
      // You are in champ select now; this is the screen you want.
      showScreen("live");
      break;

    case "locked":
      locked = status;
      selectSub = laneLabel(status.assignedPosition);
      selectPill = ["Locked", true];
      buildSlot = emptyHtml(`Looking up ${nameFor(status.championKey)}\u2026`);
      showScreen("live");
      break;

    // Champ select ended — dodged, or the game is loading. Nothing is
    // cleared: the minute after champ select is exactly when the build gets
    // read, and if a game is loading this is the build for it.
    case "left":
      selectPill = ["Connected", true];
      if (locked) {
        selectSub = `${laneLabel(locked.assignedPosition)} \u00b7 champ select ended`;
      }
      break;
  }

  paintLive();
};

/**
 * The last build the live screen was given, kept so it can be drawn again.
 *
 * The build and the reasoning about it arrive as separate events in either
 * order — you can lock in before the enemy team is visible, or after — and the
 * situational menu marks the items the engine argues for. Rendering once on
 * arrival would mean whichever came second never reached the screen.
 */
let liveBuild: LiveBuild | null = null;

/** Everything the engine currently argues, for whoever it was arguing about. */
let advice: Advice = NO_ADVICE;

const onBuild = (payload: LiveBuild): void => {
  // A build for a champion we are no longer locked into lost a race with a
  // fast swap. There is nothing to check when the game found it and we never
  // saw the champ select, which is the case this arm exists to allow.
  if (locked && payload.championKey !== locked.championKey) return;

  liveBuild = payload;
  drawLiveBuild();
};

/** Redraw the held build against the current advice. Cheap, and the only way
 *  the two events can arrive in either order without one being lost. */
const drawLiveBuild = (): void => {
  const payload = liveBuild;
  if (!payload) return;

  const lookup = payload.lookup;
  if (payload.error !== null) {
    buildSlot = badHtml(payload.error);
  } else if (lookup === null) {
    buildSlot = emptyHtml(`Nothing came back for ${nameFor(payload.championKey)}.`);
  } else if (lookup.status === "noData") {
    // The backend falls back to the key when it has no display name, so
    // prefer the roster: "no data for MonkeyKing" reads as a fault in the app
    // rather than an ordinary answer about Wukong.
    buildSlot = emptyHtml(
      `No data for ${nameFor(lookup.championKey)} ${lookup.role}. ${lookup.detail}`,
    );
  } else {
    buildSlot =
      (payload.inferredRole ? inferredNote(lookup) : "") +
      (buildHtml(lookup, advice) ??
        emptyHtml(`${lookup.champion.name} came back with an empty build.`));
  }

  paintBody();
};

/**
 * The champ-select checks. These arrive on their own schedule — before the
 * build if you locked last, and again every time one of the other nine
 * players picks — so they replace only their own slots.
 */
const onSuggestions = (payload: ChampSelectSuggestions): void => {
  threatSlot = suggestionBlock("Against this team", payload.threat, payload.itemNames);
  gapsSlot = suggestionBlock("For your team", payload.gaps, payload.itemNames);
  advice = adviceFrom(locked?.championKey ?? null, payload.threat, payload.gaps);
  drawLiveBuild();
  paintBody();
};

const onGameState = (update: InGameUpdate): void => {
  if (update.event === "noGame") {
    // This arrives on a timer whether or not the client is even open, so it
    // must touch nothing champ select owns — it is not evidence about champ
    // select. The build in particular stays: the minutes after a game are
    // exactly when someone reads what they should have built.
    //
    // What does not stay is the impression that a game is still running. The
    // standing and the in-game suggestions drop out on their own, because
    // they hang off `playing`; the header would otherwise fall back to
    // describing the client and read as though nothing had happened.
    if (!playing) return;
    lastGame = playing;
    playing = null;
    paintLive();
    return;
  }

  // Worth switching to, once. Not on every reading afterwards: somebody
  // looking something up mid-game should not be yanked back here twice a
  // minute.
  const starting = playing === null;
  playing = update;
  // The game's own reasoning replaces champ select's for the same reason the
  // threat block does: champ select is often looking at hidden seats and a
  // game never is. `state` comes first — being behind is the more urgent
  // argument about an item than the enemy composition is.
  advice = adviceFrom(update.championKey ?? null, update.state, update.threat);
  drawLiveBuild();
  if (starting) showScreen("live");
  paintLive();
};

listen<LcuStatus>("lcu:status", onStatus);
listen<LiveBuild>("live:build", onBuild);
listen<ChampSelectSuggestions>("lcu:suggestions", onSuggestions);
listen<InGameUpdate>("game:state", onGameState);

/* Icons are an enhancement, never a requirement: every tile has already drawn
   its text label by the time this resolves, and a failure leaves those in
   place. Fetched once, then applied to whatever is on screen. */
void invoke<DataDragon>("data_dragon")
  .then((catalog) => {
    icons = catalog;
    champions = catalog.champions;
    fillRoster();
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

/* ---------- self-update ----------

   Checked once, at launch. Not on a timer: a new version appearing five
   minutes into a game is not something to interrupt anybody about, and the
   next launch is soon enough.

   Every failure path here is silent. The endpoint being unreachable is
   indistinguishable from being offline, and this app is designed around
   spending most of its life beside a League client that is not running. */

interface UpdateInfo {
  version: string;
  notes: string;
}

const updateBar = el<HTMLDivElement>("update");
const updateText = el<HTMLSpanElement>("update-text");
const updateInstall = el<HTMLButtonElement>("update-install");

updateInstall.addEventListener("click", () => {
  // The button is the only route to an install, so it has to stop being one
  // the moment it is pressed — the download takes long enough to click twice.
  updateInstall.disabled = true;
  updateText.textContent = "Downloading\u2026";
  void invoke<void>("install_update").catch((error: unknown) => {
    // Failing here is worth saying, unlike failing to check: the user asked
    // for this one and is waiting on it.
    updateInstall.disabled = false;
    updateText.textContent = `Update failed: ${String(error)}`;
  });
});

void invoke<UpdateInfo | null>("check_update")
  .then((info) => {
    if (!info) return;
    updateText.textContent = `Version ${info.version} is available.`;
    updateBar.hidden = false;
  })
  .catch(() => {
    /* offline, or the manifest is unreachable; either way, not worth saying */
  });

export {};
