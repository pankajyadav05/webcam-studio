import test from "node:test";
import assert from "node:assert/strict";
import { buildFeed, isNewer, names } from "./feed.mjs";

test("the feed points darwin-aarch64 at the versioned archive", () => {
  const feed = buildFeed({ version: "0.1.0", base: "https://r2.example/", signature: "sig\n", notes: "First release.\n", pubDate: "2026-10-25T10:00:00Z" });
  assert.equal(feed.version, "0.1.0");
  assert.equal(feed.notes, "First release.");
  assert.equal(feed.platforms["darwin-aarch64"].url, "https://r2.example/releases/0.1.0/Webcam-Studio_0.1.0_aarch64.app.tar.gz");
  assert.equal(feed.platforms["darwin-aarch64"].signature, "sig");
});

test("bad input is refused", () => {
  assert.throws(() => names("v0.1"));
  assert.throws(() => buildFeed({ version: "0.1.0", base: "http://r2.example", signature: "s", notes: "", pubDate: "2026-10-25" }));
  assert.throws(() => buildFeed({ version: "0.1.0", base: "https://r2.example", signature: " ", notes: "", pubDate: "2026-10-25" }));
});

test("versions compare by number", () => {
  assert.ok(isNewer("0.1.10", "0.1.9"));
  assert.ok(isNewer("1.0.0", "0.9.9"));
  assert.ok(!isNewer("0.1.0", "0.1.0"));
  assert.ok(!isNewer("0.1.0", "0.2.0"));
});
