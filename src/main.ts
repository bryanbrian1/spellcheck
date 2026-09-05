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

/** Providers may or may not resolve names; ids are the guaranteed field. */
const tileLabel = (id: number, name?: string): string =>
  escape(name && name.trim() ? name.trim().slice(0, 9) : `#${id}`);

const tile = (id: number, name: string | undefined, cls = "tile"): string =>
  `<div class="${cls}" title="${escape(name ?? String(id))}">${tileLabel(id, name)}</div>`;

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
  const tiles = named
    .map((name) => `<div class="tile rule" title="${escape(name)}">${escape(name.slice(0, 9))}</div>`)
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

const runeBlock = (page: RunePage): string => {
  const keystone = page.primary?.[0];
  const rest = (page.primary ?? []).slice(1);
  const primary = [
    keystone === undefined ? "" : tile(keystone, undefined, "tile key"),
    ...rest.map((id) => tile(id, undefined)),
  ].join("");
  const secondary = [
    ...(page.secondary ?? []).map((id) => tile(id, undefined)),
    ...(page.shards ?? []).map((id) => tile(id, undefined, "tile sm")),
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
    `<div class="row">${set.spells.map((s) => tile(s.id, s.name)).join("")}</div>`,
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

const itemRowBlock = (title: string, groups: ItemGroup[]): string => {
  const first = groups[0];
  if (!first) return "";
  const row = first.items.map((item) => tile(item.id, item.name)).join("");
  return block(title, statLine(first.stats), `<div class="row">${row}</div>`, railFor(first.stats));
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

  // Import stays disabled: CLAUDE.md requires these to be user-initiated, and
  // the LCU layer only reads — it has no code that could write a rune page.
  importRunes.disabled = true;
  importItems.disabled = true;
};

/* ---------- interaction ---------- */

const champInput = el<HTMLInputElement>("champ");
let role: Role = "middle";
let inFlight = 0;

const lookup = async (): Promise<void> => {
  const champion = champInput.value.trim();
  if (!champion) {
    message("Type a champion and pick a role.");
    return;
  }

  const ticket = ++inFlight;
  message(`Looking up ${champion}…`);
  try {
    const result = await invoke<BuildLookup>("fetch_build", { champion, role });
    if (ticket !== inFlight) return; // a newer query already won
    render(result);
  } catch (error) {
    if (ticket !== inFlight) return;
    message(error instanceof Error ? error.message : String(error), true);
  }
};

let debounce = 0;
champInput.addEventListener("input", () => {
  window.clearTimeout(debounce);
  debounce = window.setTimeout(lookup, 350);
});
champInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    window.clearTimeout(debounce);
    void lookup();
  }
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

/** Header for a champion we have no build for yet. The key stands in until
 *  the lookup comes back with a display name — `MonkeyKing` becomes `Wukong`. */
const showLocked = (champion: LockedChampion): void => {
  selectPortrait.textContent = champion.championKey.slice(0, 2).toUpperCase();
  selectName.textContent = champion.championKey;
  selectSub.textContent =
    roleLabels[champion.assignedPosition] ?? champion.assignedPosition;
};

const showWaiting = (name: string, sub: string): void => {
  selectPortrait.textContent = "—";
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
      selectMessage(`Looking up ${status.championKey}…`);
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
    selectMessage(`Nothing came back for ${payload.champion.championKey}.`);
    return;
  }

  if (lookup.status === "noData") {
    selectName.textContent = lookup.championName;
    selectMessage(`No data for ${lookup.championName} ${lookup.role}. ${lookup.detail}`);
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

listen<LcuStatus>("lcu:status", onStatus);
listen<ChampSelectBuild>("lcu:build", onBuild);
listen<ChampSelectSuggestions>("lcu:suggestions", onSuggestions);

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
