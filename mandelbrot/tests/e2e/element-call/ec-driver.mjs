// SPDX-License-Identifier: GPL-3.0-or-later
//
// Drives a headless Element Call (SPA) for the MatrixRTC interop e2e test.
// Runs inside the mcr.microsoft.com/playwright container (playwright-core
// installed by run-interop-ec.sh, chromium from /ms-playwright).
//
// Env:
//   EC_URL       Element Call base URL (e.g. http://127.0.0.1:8080)
//   SCENARIO     "create" (EC creates + joins a call)
//                or "join" (EC joins ROOM_ID via link)
//   ROOM_ID      required for SCENARIO=join
//   VIA          via server for the room link (default synapse.m.localhost)
//   DISPLAY_NAME EC user display name (default EC-Peer)
//   OUT_DIR      writable dir for screenshots; a file $OUT_DIR/stop ends the
//                run
//   DURATION     max seconds to stay in the call (default 300)
//
// Emits machine-readable lines on stdout:
//   EVENT ready
//   EVENT createRoom <room_id>
//   EVENT request <method> <url>
//   EVENT request-body <base64 of body>   (for MatrixRTC-relevant requests)
//   EVENT joined
//   EVENT tiles <count>
//   EVENT console-error <text>
//   EVENT done

import { chromium } from "playwright-core";
import fs from "node:fs";
import path from "node:path";

const EC_URL = process.env.EC_URL ?? "http://127.0.0.1:8080";
const SCENARIO = process.env.SCENARIO ?? "create";
const ROOM_ID = process.env.ROOM_ID ?? "";
const VIA = process.env.VIA ?? "synapse.m.localhost";
const DISPLAY_NAME = process.env.DISPLAY_NAME ?? "EC-Peer";
const OUT_DIR = process.env.OUT_DIR ?? "/out";
const DURATION = Number(process.env.DURATION ?? "300");

const log = (...args) => console.log("EVENT", ...args);

function chromiumPath() {
  const root = "/ms-playwright";
  for (const dir of fs.readdirSync(root)) {
    if (dir.startsWith("chromium-")) {
      const candidates = [
        path.join(root, dir, "chrome-linux", "chrome"),
        path.join(root, dir, "chrome-linux64", "chrome"),
      ];
      for (const p of candidates) if (fs.existsSync(p)) return p;
    }
  }
  throw new Error("no chromium found in /ms-playwright");
}

const interesting = (url) =>
  url.includes("/sendToDevice/") ||
  url.includes("/keys/claim") ||
  url.includes("/keys/query") ||
  url.includes("/state/org.matrix.msc3401.call.member") ||
  url.includes("/state/m.call.member") ||
  url.includes("/state/org.matrix.msc4143.rtc.member") ||
  url.includes("msc4140") ||
  url.includes("io.element.call.encryption_keys") ||
  url.includes("m.call.encryption_keys") ||
  url.includes("rtc.notification") ||
  url.includes("sticky") ||
  url.includes("/sfu/get") ||
  url.includes("/get_token");

const browser = await chromium.launch({
  executablePath: chromiumPath(),
  args: [
    "--no-sandbox",
    "--use-fake-ui-for-media-stream",
    "--use-fake-device-for-media-stream",
    "--mute-audio",
  ],
});
const context = await browser.newContext({
  permissions: ["microphone", "camera"],
  ignoreHTTPSErrors: true,
});
const page = await context.newPage();

page.on("console", (msg) => {
  if (msg.type() === "error") log("console-error", JSON.stringify(msg.text()));
});
page.on("request", (req) => {
  const url = req.url();
  if (!interesting(url)) return;
  log("request", req.method(), url);
  const body = req.postData();
  if (body) log("request-body", Buffer.from(body).toString("base64"));
});
page.on("response", async (res) => {
  if (res.url().includes("/createRoom") && res.status() === 200) {
    try {
      const json = await res.json();
      if (json.room_id) log("createRoom", json.room_id);
    } catch {}
  }
});

const shot = (name) =>
  page
    .screenshot({ path: path.join(OUT_DIR, `${name}.png`) })
    .catch(() => {});

log("ready");
try {
  if (SCENARIO === "create") {
    await page.goto(EC_URL, { waitUntil: "load" });
    await shot("01-home");
    await page.getByTestId("home_callName").fill("interop", { timeout: 30000 });
    await page.getByTestId("home_displayName").fill(DISPLAY_NAME);
    await page.getByTestId("home_go").click();
  } else {
    const link = `${EC_URL}/room/#/?roomId=${encodeURIComponent(ROOM_ID)}&viaServers=${VIA}`;
    log("link", link);
    await page.goto(link, { waitUntil: "load" });
    await shot("01-link");
    // Unauthenticated: EC asks for a display name and registers a user.
    const nameField = page.getByTestId("joincall_displayName");
    if (await nameField.isVisible({ timeout: 15000 }).catch(() => false)) {
      await nameField.fill(DISPLAY_NAME);
      await page.getByTestId("joincall_joincall").click();
    }
  }

  // Lobby: join the call.
  await shot("02-lobby");
  await page.getByTestId("lobby_joinCall").click({ timeout: 60000 });
  await shot("03-joining");
  log("joined");
} catch (error) {
  log("error", JSON.stringify(String(error)));
  await shot("error");
  await browser.close();
  process.exit(1);
}

// In-call loop: report the number of video tiles until stopped.
const deadline = Date.now() + DURATION * 1000;
let lastTiles = -1;
let shots = 0;
while (Date.now() < deadline && !fs.existsSync(path.join(OUT_DIR, "stop"))) {
  try {
    const tiles = await page.locator('[data-testid="videoTile"]').count();
    if (tiles !== lastTiles) {
      lastTiles = tiles;
      log("tiles", tiles);
      await shot(`tiles-${String(++shots).padStart(2, "0")}-${tiles}`);
    }
  } catch (error) {
    log("error", JSON.stringify(String(error)));
    break;
  }
  await new Promise((r) => setTimeout(r, 1000));
}

await shot("99-final");
log("done");
await browser.close();
