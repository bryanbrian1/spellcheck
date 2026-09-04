// A stand-in for the League client, for developing without a game running.
//
// The LCU layer is the one part of this app that cannot be exercised by a unit
// test: it needs a TLS server on loopback with a self-signed certificate, HTTP
// Basic auth from a lockfile, and a WebSocket that speaks the client's own
// frame format. This is that server, in about a hundred lines of Node with no
// dependencies. It is a development tool and never ships — nothing in src/ or
// src-tauri/ knows it exists.
//
//   node scripts/fake-lcu.mjs
//
// It prints the command to start the app against it, then cycles forever:
// connected, in champ select, locked, left, offline, and back — so every state
// the champ select screen can show goes past about once a minute.
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

// Everything this writes — key, certificate, lockfile — is throwaway and lives
// outside the repository. A self-signed key is not something to commit.
const DIR = path.join(os.tmpdir(), "leaguechecker-fake-lcu");
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

/** `championId` is 0 until the pick completes, which is how the app tells
 *  hovering from locking. */
const session = (championId, position, phase) => ({
  localPlayerCellId: 2,
  myTeam: [
    { cellId: 0, championId: 0, assignedPosition: "top" },
    { cellId: 2, championId, championPickIntent: 0, assignedPosition: position },
  ],
  timer: { phase },
});

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

  await wait(4000);
  log('push: champ select opened   -> screen: "Pick your champion"');
  push("Create", session(0, champion.position, "PLANNING"));

  await wait(5000);
  log(`push: ${champion.name} locked ${champion.position}   -> screen: the build`);
  push("Update", session(champion.id, champion.position, "FINALIZATION"));

  await wait(2000);
  log("push: the same lock again   -> must NOT look it up twice");
  push("Update", session(champion.id, champion.position, "FINALIZATION"));

  await wait(12000);
  log('push: champ select ended    -> screen: build stays, pill "Connected"');
  push("Delete", null);

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
  process.exit(0);
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);

server.listen(PORT, "127.0.0.1", () => {
  fs.writeFileSync(LOCKFILE, `LeagueClient:4242:${PORT}:${PASSWORD}:https`);
  log(`stand-in League client listening on https://127.0.0.1:${PORT}`);
  console.log(`\n  Start the app against it with:\n\n    LEAGUECHECKER_LOCKFILE=${LOCKFILE} npm run dev\n`);
  log("cycling: connected -> in select -> locked -> left -> offline -> repeat");
});
