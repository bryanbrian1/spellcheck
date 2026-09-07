// A stand-in for the League client, for developing without a game running.
//
// The LCU layer is the one part of this app that cannot be exercised by a unit
// test: it needs a TLS server on loopback with a self-signed certificate, HTTP
// Basic auth from a lockfile, and a WebSocket that speaks the client's own
// frame format. This is that server, in about a hundred lines of Node with no
// dependencies. It is a development tool and never ships — nothing in src/ or
// src-tauri/ knows it exists.
//
//   node scripts/fake-lcu.mjs              # the whole cycle, from champ select
//   node scripts/fake-lcu.mjs --mid-game   # a game already in progress
//
// It prints the command to start the app against it, then cycles forever:
// connected, in champ select, locked, in game, left, offline, and back — so
// every state the app can show goes past about once a cycle.
//
// It also stands in for the *game*, on the fixed port Riot's Live Client Data
// API uses, so the in-game screen can be watched without playing a match. That
// server only listens while the fake game is running, which is exactly how the
// real one behaves — and it is why the app must treat a refused connection as
// "no game" rather than as a failure.
//
// `--mid-game` exists because the app supports a route nothing could exercise.
// Opening spellcheck while a match is already running is an ordinary way to
// use it, and it is the one path whose champion does not come from the client:
// champ select hands over the Data Dragon key, while a game in progress offers
// only the live API's display name. The ordinary scenario always opens with
// champ select, so that route had never been driven by anything — not a test,
// not this stand-in. In this mode the client is up, the game is already
// running, and no champ select ever happens.
//
// What it proves, and a real client would too: the app subscribes rather than
// polls, treats a 404 session as "not in champ select", resolves a championId
// to the key a provider looks builds up by, ignores a repeated lock instead of
// looking it up twice, and finds the client again on its own after it quits.

import https from "node:https";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";

const PORT = 52519;
const PASSWORD = "fake-lcu-password";

// Riot's, not ours: the Live Client Data API is always on this port, so a
// stand-in has to use it too. If a real game is running, binding fails and we
// carry on with the champ select half rather than dying.
const LIVE_PORT = 2999;

// Everything this writes — key, certificate, lockfile — is throwaway and lives
// outside the repository. A self-signed key is not something to commit.
const DIR = path.join(os.tmpdir(), "spellcheck-fake-lcu");
const LOCKFILE = path.join(DIR, "lockfile");
const KEY = path.join(DIR, "key.pem");
const CERT = path.join(DIR, "cert.pem");

const AUTH = "Basic " + Buffer.from(`riot:${PASSWORD}`).toString("base64");

/** The three cover the cases worth covering: an ordinary champion, one whose
 *  key is not its name, and a support. */
const CHAMPIONS = [
  { id: 103, alias: "Ahri", name: "Ahri", position: "middle" },
  { id: 62, alias: "MonkeyKing", name: "Wukong", position: "jungle" },
  { id: 412, alias: "Thresh", name: "Thresh", position: "utility" },
];

/** A game already in progress, and no champ select before it. */
const MID_GAME = process.argv.includes("--mid-game");

const log = (...parts) => console.log(new Date().toTimeString().slice(0, 8), ...parts);
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** The real client's certificate is self-signed too, which is exactly why the
 *  app disables verification for loopback. Regenerated when it is missing. */
const ensureCertificate = () => {
  fs.mkdirSync(DIR, { recursive: true });
  if (fs.existsSync(KEY) && fs.existsSync(CERT)) return;
  log("generating a self-signed certificate for 127.0.0.1");
  execFileSync("openssl", [
    "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "365",
    "-keyout", KEY, "-out", CERT, "-subj", "/CN=127.0.0.1",
  ], { stdio: "ignore" });
};

/* ---------- the REST API, such as we need of it ---------- */

const serve = (req, res) => {
  const authorized = req.headers.authorization === AUTH;
  log("GET", req.url, authorized ? "(auth ok)" : "(WRONG CREDENTIAL)");

  const asset = req.url.match(/\/lol-game-data\/assets\/v1\/champions\/(\d+)\.json$/);
  const champion = asset && CHAMPIONS.find((c) => c.id === Number(asset[1]));
  if (champion) {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ id: champion.id, name: champion.name, alias: champion.alias }));
    return;
  }

  // Nobody is in champ select when the app first connects. The client answers
  // 404 for a session that is not happening, and so do we.
  res.writeHead(404).end();
};

/* ---------- the event socket ---------- */

/** Server frames are never masked, and nothing here is long enough to need the
 *  64-bit length form. */
const frame = (text) => {
  const payload = Buffer.from(text);
  const n = payload.length;
  const head = n < 126
    ? Buffer.from([0x81, n])
    : Buffer.concat([Buffer.from([0x81, 126]), (() => {
        const b = Buffer.alloc(2);
        b.writeUInt16BE(n);
        return b;
      })()]);
  return Buffer.concat([head, payload]);
};

/** Client frames are always masked. */
const unmask = (buf) => {
  const offset = (buf[1] & 0x7f) === 126 ? 4 : 2;
  const mask = buf.subarray(offset, offset + 4);
  return Buffer.from(buf.subarray(offset + 4).map((b, i) => b ^ mask[i % 4])).toString();
};

const sessionEvent = (eventType, data) =>
  JSON.stringify([8, "OnJsonApiEvent_lol-champ-select_v1_session", {
    uri: "/lol-champ-select/v1/session",
    eventType,
    data,
  }]);

/** Our other four. Chosen so nothing on this side can hold a front line,
 *  which is the one team gap the second check can put an item behind. */
const ALLIES = [
  { cellId: 0, championId: 157, assignedPosition: "top" },      // Yasuo
  { cellId: 1, championId: 141, assignedPosition: "jungle" },   // Kayn
  { cellId: 3, championId: 51, assignedPosition: "bottom" },    // Caitlyn
  { cellId: 4, championId: 16, assignedPosition: "utility" },   // Soraka
];

/** The enemy, in lock order. Darius and Warwick heal, Warwick and Zed dive,
 *  and four of the five bring hard crowd control — between them they trip
 *  every rule the first check has. */
const ENEMIES = [122, 19, 238, 119, 89];

/** The same five, as the live game names them: display names and lanes. The
 *  lane assignments are what give every one of our three champions someone
 *  standing opposite them. */
const ENEMY_LIVE = [
  { name: "Darius", position: "TOP" },
  { name: "Warwick", position: "JUNGLE" },
  { name: "Zed", position: "MIDDLE" },
  { name: "Draven", position: "BOTTOM" },
  { name: "Leona", position: "UTILITY" },
];

const ALLY_LIVE = [
  { name: "Yasuo", position: "TOP" },
  { name: "Kayn", position: "JUNGLE" },
  { name: "Caitlyn", position: "BOTTOM" },
  { name: "Soraka", position: "UTILITY" },
];

/** `championId` is 0 until the pick completes, which is how the app tells
 *  hovering from locking. Seats nobody has picked yet are sent as zero
 *  rather than omitted, exactly as the real client does — that is what lets
 *  the app tell a hidden enemy team from a small one. */
const session = (championId, position, phase, enemiesShown = 0) => ({
  localPlayerCellId: 2,
  myTeam: [
    ...ALLIES,
    { cellId: 2, championId, championPickIntent: 0, assignedPosition: position },
  ],
  theirTeam: ENEMIES.map((id, seat) => ({
    cellId: 5 + seat,
    championId: seat < enemiesShown ? id : 0,
    assignedPosition: "",
  })),
  timer: { phase },
});

/* ---------- the fake game ---------- */

/** Mutated by the scenario; read by every request to the live server. */
let live = null;

// The real payload's item shape, all nine fields, and the two that matter are
// the ones this stand-in used to get wrong.
//
// `itemID` capitalises both letters — it is the only field in the payload
// shaped that way, and a parser deriving it from camelCase silently reads zero.
//
// `price` is the *combine* cost, not what the item is worth: a finished
// Rabadon's Deathcap reports 1100 against a real 3500, and finished boots
// report zero. This stand-in used to mint a price the scenario chose, which is
// how the app spent months summing combine costs while every test agreed with
// it. Ids here are real, and what they cost is data/meta/items.json's business.
const HELD = [
  { itemID: 3020, price: 350, slot: 0, displayName: "Sorcerer's Shoes" },
  { itemID: 3165, price: 800, slot: 1, displayName: "Morellonomicon" },
  { itemID: 3157, price: 1000, slot: 2, displayName: "Zhonya's Hourglass" },
  { itemID: 3089, price: 1100, slot: 3, displayName: "Rabadon's Deathcap" },
];

const item = (entry, count = 1) => ({
  canUse: false,
  consumable: false,
  count,
  displayName: entry.displayName,
  itemID: entry.itemID,
  price: entry.price,
  rawDescription: "GeneratedTip_Item_" + entry.itemID + "_Description",
  rawDisplayName: "Item_" + entry.itemID + "_Name",
  slot: entry.slot,
});

/** A plausible inventory. `depth` picks how far into the build they are. */
const inventory = (depth) => HELD.slice(0, Math.max(0, Math.min(depth, HELD.length))).map((e) => item(e));

const livePlayer = (name, team, position, gold, level, dead = false) => ({
  championName: name,
  team,
  position,
  level,
  isDead: dead,
  respawnTimer: dead ? 12.5 : 0,
  riotId: `${name}#EUW`,
  summonerName: name,
  // `gold` is the scenario's shorthand for "how far along are they", not a
  // number the payload carries: the live API never reports what a player has
  // spent, only what they hold.
  items: inventory(Math.round(gold / 1200)),
  scores: { kills: 2, deaths: 4, assists: 3, creepScore: 118, wardScore: 9 },
});

const allGameData = () => ({
  activePlayer: {
    riotId: `${live.champion.name}#EUW`,
    summonerName: live.champion.name,
    currentGold: live.goldInHand,
    level: live.level,
  },
  allPlayers: [
    livePlayer(live.champion.name, "ORDER", live.position, live.ourGold, live.level, live.dead),
    ...ALLY_LIVE.map((ally) => livePlayer(ally.name, "ORDER", ally.position, 4000, 11)),
    ...ENEMY_LIVE.map((enemy) =>
      livePlayer(
        enemy.name,
        "CHAOS",
        enemy.position,
        // Only the player in our lane pulls ahead; the rest stay level, so
        // the standing is unmistakably about one opponent.
        enemy.position === live.position ? live.theirGold : 4200,
        enemy.position === live.position ? live.level + live.levelGap : 11,
      ),
    ),
  ],
  gameData: { gameTime: live.gameTime, gameMode: "CLASSIC", mapName: "Map11", mapNumber: 11 },
  events: { Events: [{ EventID: 0, EventName: "GameStart", EventTime: 0 }] },
});

/** Only listening while a fake game is running, which is the whole point: the
 *  app has to read a refused connection as "no game" rather than a failure. */
let liveServer = null;

const startGame = (champion) => {
  live = {
    champion,
    // Champ select spells positions in lower case and the live API in upper.
    position: champion.position.toUpperCase(),
    gameTime: 615,
    level: 11,
    levelGap: 0,
    ourGold: 4000,
    theirGold: 4200,
    goldInHand: 350,
    dead: false,
  };

  liveServer = https.createServer(
    { key: fs.readFileSync(KEY), cert: fs.readFileSync(CERT) },
    (req, res) => {
      if (req.url && req.url.startsWith("/liveclientdata/allgamedata")) {
        // Logged because the whole question about the in-game route is
        // whether the app ever asks. Silence here is the symptom.
        log("GET /liveclientdata/allgamedata");
        const body = JSON.stringify(allGameData());
        res.writeHead(200, { "content-type": "application/json" });
        res.end(body);
        return;
      }
      res.writeHead(404, { "content-type": "application/json" });
      res.end("{}");
    },
  );

  liveServer.on("error", (error) => {
    log(`live server could not bind ${LIVE_PORT} (${error.code}) — a real game is probably running`);
    liveServer = null;
    live = null;
  });

  liveServer.listen(LIVE_PORT, "127.0.0.1", () =>
    log(`the game starts             -> live data on https://127.0.0.1:${LIVE_PORT}`),
  );
};

const stopGame = () => {
  live = null;
  if (liveServer) {
    liveServer.close();
    liveServer = null;
  }
};

let round = 0;

const upgrade = async (req, socket) => {
  const champion = CHAMPIONS[round++ % CHAMPIONS.length];
  log("UPGRADE", req.headers.authorization === AUTH ? "(auth ok)" : "(WRONG CREDENTIAL)");

  const accept = crypto
    .createHash("sha1")
    .update(req.headers["sec-websocket-key"] + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")
    .digest("base64");
  socket.write(
    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
      `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
  );

  socket.on("data", (buf) => {
    if ((buf[0] & 0x0f) === 0x1) log("CLIENT SENT", unmask(buf));
  });
  socket.on("error", () => {});

  const push = (eventType, data) => {
    if (!socket.destroyed) socket.write(frame(sessionEvent(eventType, data)));
  };

  if (MID_GAME) {
    // Nothing to push. The client is connected and idle — exactly what it
    // looks like from the app's side when a match is already under way — and
    // everything the screen shows has to come from the live API alone.
    log("mid-game mode: no champ select will happen");
    log("expect: the app finds the running game on its own and shows a build");
    return;
  }

  await wait(4000);
  log('push: champ select opened   -> screen: "Pick your champion"');
  push("Create", session(0, champion.position, "PLANNING"));

  await wait(5000);
  log(`push: ${champion.name} locked ${champion.position}   -> screen: the build`);
  push("Update", session(champion.id, champion.position, "FINALIZATION"));

  await wait(2000);
  log("push: the same lock again   -> must NOT look it up twice");
  push("Update", session(champion.id, champion.position, "FINALIZATION"));

  // The enemy team fills in one seat at a time, which is what champ select
  // actually looks like. None of these may trigger a second build lookup:
  // our champion has not changed, only the composition around it.
  for (let shown = 1; shown <= ENEMIES.length; shown++) {
    await wait(2000);
    log(
      `push: enemy ${shown} of ${ENEMIES.length} locks in` +
        (shown < 3
          ? "  -> too few to call a split; expect silence"
          : "  -> suggestions, and NO second build lookup"),
    );
    push("Update", session(champion.id, champion.position, "FINALIZATION", shown));
  }

  await wait(12000);
  log('push: champ select ended    -> screen: build stays, pill "Connected"');
  push("Delete", null);

  // The game itself. Phases are long because the app deliberately looks
  // rarely — every thirty seconds while alive — so a shorter phase would end
  // before it was ever sampled. That slowness is the feature being tested.
  startGame(champion);

  await wait(35000);
  log("you fall behind             -> expect a red banner and a cheap resist");
  Object.assign(live, { ourGold: 4200, theirGold: 9000, levelGap: 2, gameTime: 1100, goldInHand: 1340 });

  await wait(35000);
  log("you die                     -> the watcher speeds up to every five seconds");
  Object.assign(live, { dead: true, gameTime: 1180 });

  await wait(20000);
  log("you respawn, further behind -> the advice should not have changed its mind");
  Object.assign(live, { dead: false, ourGold: 5200, theirGold: 11800, levelGap: 3, gameTime: 1500 });

  await wait(35000);
  log('the game ends               -> screen: pill "Game over", sub "· game ended",');
  log('                               the standing goes, the build stays');
  stopGame();

  await wait(6000);
  log('the client quits            -> screen: "Offline"');
  socket.destroy();
  fs.rmSync(LOCKFILE, { force: true });

  await wait(8000);
  log("the client restarts         -> the app should find it on its own");
  fs.writeFileSync(LOCKFILE, `LeagueClient:4242:${PORT}:${PASSWORD}:https`);
};

/* ---------- run ---------- */

ensureCertificate();

const server = https.createServer(
  { key: fs.readFileSync(KEY), cert: fs.readFileSync(CERT) },
  serve,
);
server.on("upgrade", upgrade);

const shutdown = () => {
  fs.rmSync(LOCKFILE, { force: true });
  stopGame();
  process.exit(0);
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);

server.listen(PORT, "127.0.0.1", () => {
  fs.writeFileSync(LOCKFILE, `LeagueClient:4242:${PORT}:${PASSWORD}:https`);
  log(`stand-in League client listening on https://127.0.0.1:${PORT}`);
  console.log(`\n  Start the app against it with:\n\n    SPELLCHECK_LOCKFILE=${LOCKFILE} npm run dev\n`);

  if (MID_GAME) {
    // Before the app is even started, so that whenever it connects the match
    // is already under way and there is no champ select to have missed.
    startGame(CHAMPIONS[0]);
    log("mid-game: a match is already running; start the app now");
    return;
  }

  log("cycling: connected -> in select -> locked -> in game -> offline -> repeat");
});
