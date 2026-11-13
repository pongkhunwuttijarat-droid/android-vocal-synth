import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const transportSource = readFileSync(new URL("./curve-stroke-transport.js", import.meta.url), "utf8");
const {
  commitCurveStrokeInWasm,
  commitSimplifiedCurveStroke,
  readCurveStrokeContextInWasm,
  reduceCurveStroke,
} = await import(
  `data:text/javascript;base64,${Buffer.from(transportSource).toString("base64")}`,
);

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const bindingPath = process.env.T009_WASM_BINDING;
assert.ok(bindingPath, "Set T009_WASM_BINDING to wasm-bindgen's Node.js output module");
const nativeOracle = process.env.T009_NATIVE_ORACLE;
assert.ok(nativeOracle, "Set T009_NATIVE_ORACLE to the built native curve_stroke_parity_oracle example");
const { Engine } = createRequire(import.meta.url)(bindingPath);
const fixturePath = resolve(root, "native/crates/project-session/tests/vectors/curve_adapter_shared.ustx");
const fixture = readFileSync(fixturePath, "utf8");
const noCurvesFixture = fixture.replace(
  "  curves:\n  - abbr: dyn\n    xs: [0, 480]\n    ys: [0, 10]\n",
  "",
);
assert.notEqual(noCurvesFixture, fixture, "expected fixture curve removal to match exactly");

function fixtureWithRange(yaml, min, max) {
  return yaml.replace("    min: -240.0", `    min: ${min}.0`).replace("    max: 120.0", `    max: ${max}.0`);
}

function open(yaml) {
  const engine = new Engine();
  const snapshot = JSON.parse(engine.session_open_yaml(yaml, ""));
  const part = snapshot.project.tracks[0].parts[0];
  return { engine, part };
}

function pointsFor(engine, abbr = "dyn") {
  const snapshot = JSON.parse(engine.session_snapshot_json());
  return snapshot.project.tracks[0].parts[0].curves.find(curve => curve.abbr === abbr)?.points ?? [];
}

function compareRawAndReduced({ yaml, startTick, endTick, rawPoints, create = true, expectedTolerance }) {
  const rawState = open(yaml);
  const reducedState = open(yaml);
  const rawCurve = rawState.part.curves.find(curve => curve.abbr === "dyn");
  const rawContext = readCurveStrokeContextInWasm(rawState.engine, {
    partId: rawState.part.id,
    curveId: rawCurve?.id ?? null,
    abbr: "dyn",
    create,
  });
  const context = readCurveStrokeContextInWasm(reducedState.engine, {
    partId: reducedState.part.id,
    curveId: reducedState.part.curves.find(curve => curve.abbr === "dyn")?.id ?? null,
    abbr: "dyn",
    create,
  });
  if (expectedTolerance !== undefined) assert.equal(context.descriptor.simplify_tolerance, expectedTolerance);
  const rawCommand = {
    type: "curve.apply_stroke",
    part_id: rawState.part.id,
    curve_id: create ? null : rawCurve.id,
    abbr: "dyn",
    create,
    mode: "replace",
    start_tick: startTick,
    end_tick: endTick,
    points: rawPoints,
  };
  const reducedCommand = { ...rawCommand, part_id: reducedState.part.id };
  if (!create) reducedCommand.curve_id = reducedState.part.curves.find(curve => curve.abbr === "dyn").id;
  const reducedPoints = reduceCurveStroke({
    basePoints: context.points,
    startTick,
    endTick,
    rawPoints,
    minValue: context.descriptor.min_value,
    maxValue: context.descriptor.max_value,
    defaultValue: context.descriptor.default_value,
    tolerance: context.descriptor.simplify_tolerance,
  });
  const rawOutcome = JSON.parse(rawState.engine.session_apply_v2(JSON.stringify({
    expected_revision: rawContext.revision,
    command: rawCommand,
  })));
  const reducedOutcome = JSON.parse(reducedState.engine.session_apply_v2(JSON.stringify({
    expected_revision: context.revision,
    command: { ...reducedCommand, points: reducedPoints },
  })));
  assert.equal(rawOutcome.revision, reducedOutcome.revision);
  assert.deepEqual(pointsFor(rawState.engine), pointsFor(reducedState.engine));
  return { context, reducedPoints, resultPoints: pointsFor(rawState.engine) };
}

test("JS curve stroke reduction matches native and direct-WASM Rust commits", () => {
  const cases = [
    {
      name: "below descriptor tolerance",
      yaml: noCurvesFixture,
      expectedTolerance: 1.8,
      startTick: 0,
      endTick: 100,
      rawPoints: [{ tick: 25, value: 1 }],
    },
    {
      name: "above descriptor tolerance",
      yaml: noCurvesFixture,
      expectedTolerance: 1.8,
      startTick: 0,
      endTick: 100,
      rawPoints: [{ tick: 25, value: 2 }],
    },
    {
      name: "exactly at capped tolerance",
      yaml: fixtureWithRange(noCurvesFixture, -1000, 1000),
      expectedTolerance: 5,
      startTick: 0,
      endTick: 10,
      rawPoints: [{ tick: 5, value: 5 }],
    },
    {
      name: "above capped tolerance",
      yaml: fixtureWithRange(noCurvesFixture, -1000, 1000),
      expectedTolerance: 5,
      startTick: 0,
      endTick: 10,
      rawPoints: [{ tick: 5, value: 6 }],
    },
    {
      name: "equal-distance tie and uneven spacing",
      yaml: noCurvesFixture,
      expectedTolerance: 1.8,
      startTick: 0,
      endTick: 20,
      rawPoints: [{ tick: 5, value: 5 }, { tick: 10, value: 5 }],
    },
    {
      name: "negative values and multiple retained intervals",
      yaml: fixtureWithRange(noCurvesFixture, -120, 120),
      expectedTolerance: 1.2,
      startTick: 0,
      endTick: 100,
      rawPoints: [
        { tick: 15, value: -12 },
        { tick: 35, value: -2 },
        { tick: 60, value: 14 },
        { tick: 80, value: -10 },
      ],
    },
    {
      name: "existing curve rebase with outside controls",
      yaml: fixture,
      expectedTolerance: 1.8,
      create: false,
      startTick: 240,
      endTick: 720,
      rawPoints: [
        { tick: 240, value: 40 },
        { tick: 360, value: 25 },
        { tick: 480, value: -20 },
        { tick: 600, value: 30 },
        { tick: 720, value: 5 },
      ],
    },
  ];

  const nativeVectors = [];
  for (const vector of cases) {
    const result = compareRawAndReduced(vector);
    assert.ok(result.reducedPoints.length > 0, `${vector.name}: replace cannot send an empty point list`);
    assert.ok(result.reducedPoints.length <= vector.rawPoints.length, `${vector.name}: reducer must not expand stroke payload`);
    if (vector.name === "exactly at capped tolerance") {
      assert.deepEqual(result.reducedPoints, [{ tick: 5, value: 5 }], "equality is removed from the committed full curve but replace keeps one legal point");
    }
    nativeVectors.push({
      ...vector,
      reducedPoints: result.reducedPoints,
      expectedPoints: result.resultPoints,
    });
  }

  const nativeRun = spawnSync(nativeOracle, [], {
    input: JSON.stringify(nativeVectors),
    encoding: "utf8",
  });
  assert.equal(nativeRun.status, 0, `native oracle failed: ${nativeRun.stderr}`);
  const nativeResults = JSON.parse(nativeRun.stdout);
  assert.equal(nativeResults.length, nativeVectors.length);
  for (let index = 0; index < nativeVectors.length; index += 1) {
    assert.deepEqual(nativeResults[index].raw, nativeResults[index].reduced,
      `${nativeVectors[index].name}: native raw/reduced results differ`);
    assert.deepEqual(nativeResults[index].raw, nativeVectors[index].expectedPoints,
      `${nativeVectors[index].name}: native and direct-WASM results differ`);
  }
});

test("WASM transport commits create and existing strokes as one Session revision", async () => {
  const created = open(noCurvesFixture);
  const createCommand = {
    type: "curve.apply_stroke",
    part_id: created.part.id,
    curve_id: null,
    abbr: "dyn",
    create: true,
    mode: "replace",
    start_tick: 0,
    end_tick: 100,
    points: [{ tick: 25, value: 8 }, { tick: 50, value: -4 }, { tick: 75, value: 6 }],
  };
  const createdOutcome = await commitCurveStrokeInWasm(created.engine, {
    command: createCommand,
    gestureRevision: 0,
  });
  assert.equal(createdOutcome.revision, 1);
  const committedPoints = pointsFor(created.engine);
  assert.ok(committedPoints.length > 0);
  assert.equal(JSON.parse(created.engine.session_undo_v2(1n)).revision, 2);
  assert.deepEqual(pointsFor(created.engine), [], "one undo removes the created curve exactly");
  assert.equal(JSON.parse(created.engine.session_redo_v2(2n)).revision, 3);
  assert.deepEqual(pointsFor(created.engine), committedPoints, "one redo restores the committed curve");

  const existing = open(fixture);
  const curve = existing.part.curves.find(item => item.abbr === "dyn");
  const existingCommand = {
    type: "curve.apply_stroke",
    part_id: existing.part.id,
    curve_id: curve.id,
    create: false,
    mode: "replace",
    start_tick: 240,
    end_tick: 720,
    points: [{ tick: 240, value: 40 }, { tick: 480, value: 10 }, { tick: 720, value: -3 }],
  };
  const existingOutcome = await commitCurveStrokeInWasm(existing.engine, {
    command: existingCommand,
    gestureRevision: 0,
  });
  assert.equal(existingOutcome.revision, 1);
});

test("revision conflict re-reads Rust state and never falls back to raw points", async () => {
  const command = {
    type: "curve.apply_stroke",
    part_id: "part-id",
    curve_id: "curve-id",
    abbr: "dyn",
    mode: "replace",
    start_tick: 20,
    end_tick: 80,
    points: [
      { tick: 25, value: 24 },
      { tick: 35, value: -16 },
      { tick: 45, value: -7 },
      { tick: 55, value: -18 },
      { tick: 65, value: -87 },
      { tick: 75, value: -57 },
    ],
  };
  const descriptor = { min_value: -1000, max_value: 1000, default_value: 0, simplify_tolerance: 5 };
  const oldBase = [];
  const latestBase = [
    { tick: 0, value: 0 },
    { tick: 20, value: 40 },
    { tick: 80, value: 80 },
    { tick: 100, value: 100 },
  ];
  const expectedLatest = reduceCurveStroke({
    basePoints: latestBase,
    startTick: command.start_tick,
    endTick: command.end_tick,
    rawPoints: command.points,
    minValue: descriptor.min_value,
    maxValue: descriptor.max_value,
    defaultValue: descriptor.default_value,
    tolerance: descriptor.simplify_tolerance,
  });
  const expectedOld = reduceCurveStroke({
    basePoints: oldBase,
    startTick: command.start_tick,
    endTick: command.end_tick,
    rawPoints: command.points,
    minValue: descriptor.min_value,
    maxValue: descriptor.max_value,
    defaultValue: descriptor.default_value,
    tolerance: descriptor.simplify_tolerance,
  });
  assert.notDeepEqual(expectedLatest, expectedOld, "vector must expose stale-base reduction");

  let reads = 0;
  let writes = 0;
  const result = await commitSimplifiedCurveStroke({
    command,
    gestureRevision: 4,
    maxAttempts: 2,
    readLatest: async () => {
      reads += 1;
      return {
        revision: reads === 1 ? 4 : 5,
        part_id: command.part_id,
        curve_id: command.curve_id,
        abbr: command.abbr,
        points: reads === 1 ? oldBase : latestBase,
        descriptor,
      };
    },
    apply: async request => {
      writes += 1;
      if (writes === 1) throw { code: "revision_conflict" };
      return request;
    },
  });
  assert.equal(reads, 2);
  assert.equal(writes, 2);
  assert.equal(result.expected_revision, 5);
  assert.deepEqual(result.command.points, expectedLatest);
  assert.notDeepEqual(result.command.points, command.points, "raw samples must never be sent as fallback");
});
