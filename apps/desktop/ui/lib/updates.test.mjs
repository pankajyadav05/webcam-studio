import test from "node:test";
import assert from "node:assert/strict";
import { describeUpdate } from "./updates.js";

test("a ready update offers Restart to Update", () => {
  const view = describeUpdate({ state: "ready", version: "0.2.0" });
  assert.equal(view.action, "restart");
  assert.equal(view.label, "Restart to Update");
  assert.match(view.text, /0\.2\.0/);
});

test("busy states offer no button", () => {
  assert.equal(describeUpdate({ state: "checking" }).action, null);
  assert.equal(describeUpdate({ state: "downloading", version: "0.2.0" }).action, null);
});

test("a failure says why and offers to try again", () => {
  const view = describeUpdate({ state: "failed", message: "Offline." });
  assert.equal(view.action, "check");
  assert.match(view.text, /Offline\./);
  assert.ok(view.warn);
});

test("a build without updates shows nothing", () => {
  assert.equal(describeUpdate({ state: "disabled" }), null);
});
