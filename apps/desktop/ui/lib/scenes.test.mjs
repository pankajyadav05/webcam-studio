import test from "node:test";
import assert from "node:assert/strict";
import { addScene, removeScene, activeScene, scenePatch, cleanScenes, describeLook, MAX_SCENES } from "./scenes.js";

const state = (over = {}) => ({
  zoom: 2, mirror: true, autoFrame: false,
  background: { mode: "blur", radius: 18, preset: "studio", custom: null, cutoff: 0.5 },
  appearance: { smoothing: 0.4, low_light: false, warmth: 0, studio_light: 0 },
  ...over,
});

test("a saved scene is found again and put back without zoom or mirror", () => {
  const scenes = addScene([], " Client call ", state(), "a");
  assert.equal(scenes[0].name, "Client call");
  assert.equal(activeScene(scenes, state()), "a");
  assert.equal(activeScene(scenes, state({ zoom: 1, mirror: false })), "a", "zoom and mirror do not count");
  assert.equal(activeScene(scenes, state({ autoFrame: true })), null);
  const patch = scenePatch(scenes[0]);
  assert.deepEqual(Object.keys(patch).sort(), ["appearance", "autoFrame", "background"]);
  assert.equal(patch.background.radius, 18);
  assert.equal("zoom" in patch, false);
});

test("same name replaces, removal works, and the list is capped", () => {
  let scenes = addScene([], "Standup", state(), "a");
  scenes = addScene(scenes, "standup", state({ autoFrame: true }), "b");
  assert.deepEqual(scenes.map((s) => s.id), ["b"]);
  assert.deepEqual(removeScene(scenes, "b"), []);
  assert.equal(addScene([], "   ", state()).length, 0);
  let many = [];
  for (let i = 0; i < 20; i++) many = addScene(many, `n${i}`, state(), `id${i}`);
  assert.equal(many.length, MAX_SCENES);
  assert.equal(many.at(-1).id, "id19");
});

test("stored scenes are cleaned", () => {
  const raw = [{ id: "a", name: "Ok", look: { background: { mode: "weird", radius: 900 }, appearance: { smoothing: 4 } } },
    { id: "a", name: "Duplicate" }, { id: "", name: "No id" }, { id: "c", name: "" }, null, "x"];
  const clean = cleanScenes(raw);
  assert.equal(clean.length, 1);
  assert.deepEqual(clean[0].look.background, { mode: "none", radius: 50, preset: "studio" });
  assert.equal(clean[0].look.appearance.smoothing, 1);
  assert.deepEqual(cleanScenes("nope"), []);
});

test("a scene describes itself in a few words", () => {
  assert.equal(describeLook(state().background && { background: state().background, appearance: state().appearance, autoFrame: true }), "Blur · Touch up 40% · Auto-frame");
  assert.equal(describeLook({}), "Plain camera");
});
