//! The Riot API client: routing, rate limiting, and knowing when to stop.
//!
//! Everything the crawler asks of Riot goes through here, for one reason: the
//! rate limit is the binding constraint on the whole job, so there has to be
//! exactly one place that counts. A development key allows twenty requests a
//! second and a hundred every two minutes, and it is the second of those that
//! decides how much data a run can gather — fifty a minute, whatever the
//! burst allowance says.
//!
//! Two clocks stop a run: a request budget and a wall-clock deadline. The
//! deadline matters more, because the scheduled workflow has a timeout and a
//! job killed at sixty minutes commits nothing at all. Stopping early with
//! four fifths of the data is a good run; being killed with all of it is not.
//!
//! Dependency-free on purpose — this is a data job, and `npm ci` on it would
//! install the desktop app's toolchain to make HTTP requests.

/** Match-v5 is routed by region; league-v4 and summoner-v4 by platform. */
const PLATFORM_REGIONS = {
  na1: "americas", br1: "americas", la1: "americas", la2: "americas",
  euw1: "europe", eun1: "europe", tr1: "europe", ru: "europe",
  kr: "asia", jp1: "asia",
  oc1: "sea", ph2: "sea", sg2: "sea", th2: "sea", tw2: "sea", vn2: "sea",
};

export function regionFor(platform) {
  const region = PLATFORM_REGIONS[platform];
  if (!region) {
    throw new Error(
      `unknown platform "${platform}" — expected one of ${Object.keys(PLATFORM_REGIONS).join(", ")}`,
    );
  }
  return region;
}

/** Raised when a run hits its budget or its deadline. Not a failure. */
export class OutOfBudget extends Error {
  constructor(reason) {
    super(reason);
    this.name = "OutOfBudget";
  }
}

/** Raised for a response we cannot use. Carries the status so callers can
 *  tell "this key is dead" from "that match is gone". */
export class RiotApiError extends Error {
  constructor(status, url, detail) {
    super(`${status} from ${url}${detail ? ` — ${detail}` : ""}`);
    this.name = "RiotApiError";
    this.status = status;
    this.url = url;
  }
}

/**
 * A sliding window. One per documented limit.
 *
 * Requests are timestamped as they go out and expire out of the window on
 * their own, so a burst is allowed to be a burst and the average still holds.
 */
class Window {
  constructor(count, seconds) {
    this.count = count;
    this.windowMs = seconds * 1000;
    this.hits = [];
  }

  /** Milliseconds until this window would allow another request. */
  waitMs(now) {
    while (this.hits.length > 0 && now - this.hits[0] >= this.windowMs) this.hits.shift();
    if (this.hits.length < this.count) return 0;
    return this.windowMs - (now - this.hits[0]) + 1;
  }

  record(now) {
    this.hits.push(now);
  }
}

/** `"20:1,100:120"` — the development key's documented limits, and the
 *  default. A production key overrides this through the environment. */
export function parseLimits(raw) {
  return raw
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const [count, seconds] = part.split(":").map(Number);
      if (!Number.isFinite(count) || !Number.isFinite(seconds) || count <= 0 || seconds <= 0) {
        throw new Error(`bad rate limit "${part}" — expected count:seconds, e.g. 100:120`);
      }
      return new Window(count, seconds);
    });
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export class RiotApi {
  constructor({
    key,
    platform = "na1",
    limits = "20:1,100:120",
    budget = Infinity,
    deadline = Infinity,
    timeoutMs = 10_000,
    log = () => {},
  }) {
    if (!key) throw new Error("no Riot API key");
    this.key = key;
    this.platform = platform;
    this.region = regionFor(platform);
    this.windows = parseLimits(limits);
    this.budget = budget;
    this.deadline = deadline;
    this.timeoutMs = timeoutMs;
    this.log = log;

    this.spent = 0;
    this.throttledMs = 0;
    this.retries = 0;
  }

  /** Why the run should stop, or null while it may continue. */
  stopReason() {
    if (this.spent >= this.budget) return `request budget spent (${this.budget})`;
    if (Date.now() >= this.deadline) return "deadline reached";
    return null;
  }

  get exhausted() {
    return this.stopReason() !== null;
  }

  /** Block until every window allows another request. */
  async #throttle() {
    for (;;) {
      const now = Date.now();
      const wait = Math.max(0, ...this.windows.map((window) => window.waitMs(now)));
      if (wait === 0) {
        for (const window of this.windows) window.record(now);
        return;
      }
      // Waiting past the deadline is worse than stopping: the run would sit
      // idle and then be killed with nothing written.
      if (now + wait >= this.deadline) {
        throw new OutOfBudget("deadline would pass while waiting on the rate limit");
      }
      this.throttledMs += wait;
      await sleep(wait);
    }
  }

  /**
   * One GET.
   *
   * `null` for 404, which every caller treats as "that does not exist" rather
   * than as a failure — a match can be gone, a summoner can have no ranked
   * history. Anything else that is not a success throws.
   */
  async get(url, { attempt = 0 } = {}) {
    const stop = this.stopReason();
    if (stop) throw new OutOfBudget(stop);

    await this.#throttle();
    this.spent += 1;

    let response;
    try {
      response = await fetch(url, {
        headers: { "X-Riot-Token": this.key, "Accept": "application/json" },
        signal: AbortSignal.timeout(this.timeoutMs),
      });
    } catch (error) {
      // A dropped connection is worth one retry; the network is not usually
      // broken for long, and losing the whole run to one blip is expensive.
      if (attempt < 2 && !this.exhausted) {
        this.retries += 1;
        await sleep(1000 * (attempt + 1));
        return this.get(url, { attempt: attempt + 1 });
      }
      throw new RiotApiError(0, url, error.message);
    }

    if (response.status === 404) return null;

    if (response.status === 429) {
      // Riot tells us exactly how long to wait. Believe it rather than
      // guessing, and count the wait so the run's report can show whether the
      // limiter is set too high.
      const retryAfter = Number(response.headers.get("retry-after")) || 5;
      const waitMs = Math.min(retryAfter, 120) * 1000;
      this.retries += 1;
      this.log(`rate limited, waiting ${retryAfter}s (${response.headers.get("x-rate-limit-type") ?? "unknown"} limit)`);
      if (Date.now() + waitMs >= this.deadline) {
        throw new OutOfBudget("rate limited past the deadline");
      }
      this.throttledMs += waitMs;
      await sleep(waitMs);
      return this.get(url, { attempt });
    }

    if (response.status >= 500 && attempt < 3) {
      this.retries += 1;
      await sleep(1000 * 2 ** attempt);
      return this.get(url, { attempt: attempt + 1 });
    }

    if (!response.ok) {
      const body = await response.text().catch(() => "");
      throw new RiotApiError(response.status, url, body.slice(0, 200));
    }

    return response.json();
  }

  platformGet(path) {
    return this.get(`https://${this.platform}.api.riotgames.com${path}`);
  }

  regionGet(path) {
    return this.get(`https://${this.region}.api.riotgames.com${path}`);
  }
}
