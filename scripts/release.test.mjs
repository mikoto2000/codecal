import test from "node:test";
import process from "node:process";
import { execFileSync } from "node:child_process";
import { fileURLToPath, URL } from "node:url";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import {
  parseReleaseTag,
  targets,
  applyReleaseVersion,
  collectAssets,
  verifyReleaseAssets,
} from "./release.mjs";
function fixture(run) {
  const root = mkdtempSync(path.join(tmpdir(), "codecal-release-"));
  try {
    return run(root);
  } finally {
    const base = path.resolve(tmpdir()) + path.sep;
    assert.ok(
      path.resolve(root).startsWith(base) &&
        path.basename(root).startsWith("codecal-release-"),
    );
    rmSync(root, { recursive: true, force: true });
  }
}
function project(root) {
  mkdirSync(path.join(root, "src-tauri"), { recursive: true });
  writeFileSync(
    path.join(root, "package.json"),
    JSON.stringify({
      name: "codecal",
      version: "0.1.0",
      dependencies: { react: "19" },
    }),
  );
  writeFileSync(
    path.join(root, "package-lock.json"),
    JSON.stringify({
      name: "codecal",
      version: "0.1.0",
      lockfileVersion: 3,
      packages: {
        "": { name: "codecal", version: "0.1.0" },
        "node_modules/example": { version: "8.8.8" },
      },
    }),
  );
  writeFileSync(
    path.join(root, "src-tauri", "tauri.conf.json"),
    JSON.stringify({
      productName: "CodeCal",
      version: "0.1.0",
      bundle: { active: true },
    }),
  );
  writeFileSync(
    path.join(root, "src-tauri", "Cargo.toml"),
    '[package]\nname = "codecal"\nversion = "0.1.0"\n\n[dependencies]\nexample = { version = "8.8.8" }\n',
  );
  writeFileSync(
    path.join(root, "src-tauri", "Cargo.lock"),
    'version = 4\n\n[[package]]\nname = "codecal"\nversion = "0.1.0"\n\n[[package]]\nname = "example"\nversion = "8.8.8"\n',
  );
}
test("parses stable prerelease and build metadata tags", () => {
  assert.deepEqual(parseReleaseTag("v1.2.3"), {
    version: "1.2.3",
    prerelease: false,
  });
  assert.deepEqual(parseReleaseTag("v2.0.0-rc.1+build.7"), {
    version: "2.0.0-rc.1+build.7",
    prerelease: true,
  });
});
test("rejects invalid or shell-like version tags", () => {
  for (const tag of [
    "1.2.3",
    "v1.2",
    "v01.2.3",
    "v1.2.3-01",
    "v1.2.3-beta.01",
    "v1.2.3;echo bad",
    "v1.2.3\n",
    "vnext",
  ])
    assert.throws(() => parseReleaseTag(tag));
});
test("covers all six native OS and architecture pairs", () => {
  assert.equal(targets.length, 6);
  assert.deepEqual(targets.map((t) => `${t.platform}-${t.arch}`).sort(), [
    "linux-arm64",
    "linux-x64",
    "macos-arm64",
    "macos-x64",
    "windows-arm64",
    "windows-x64",
  ]);
  assert.equal(new Set(targets.map((t) => t.target)).size, 6);
  assert.equal(
    targets.find((t) => t.target === "aarch64-pc-windows-msvc").runner,
    "windows-11-arm",
  );
  assert.equal(
    targets.find((t) => t.target === "aarch64-unknown-linux-gnu").runner,
    "ubuntu-22.04-arm",
  );
});
test("updates all five version files and preserves dependency versions", () =>
  fixture((root) => {
    project(root);
    applyReleaseVersion(root, "v1.2.3-rc.1");
    assert.equal(
      JSON.parse(readFileSync(path.join(root, "package.json"))).version,
      "1.2.3-rc.1",
    );
    const lock = JSON.parse(readFileSync(path.join(root, "package-lock.json")));
    assert.equal(lock.version, "1.2.3-rc.1");
    assert.equal(lock.packages[""].version, "1.2.3-rc.1");
    assert.equal(lock.packages["node_modules/example"].version, "8.8.8");
    assert.equal(
      JSON.parse(readFileSync(path.join(root, "src-tauri", "tauri.conf.json")))
        .version,
      "1.2.3-rc.1",
    );
    for (const name of ["Cargo.toml", "Cargo.lock"]) {
      const s = readFileSync(path.join(root, "src-tauri", name), "utf-8");
      assert.match(s, /version = "1.2.3-rc.1"/);
      assert.match(s, /version = "8.8.8"/);
    }
  }));
test("validates every source before writing any version changes", () =>
  fixture((root) => {
    project(root);
    writeFileSync(path.join(root, "src-tauri", "Cargo.lock"), "broken");
    assert.throws(() => applyReleaseVersion(root, "v1.2.3"));
    assert.equal(
      JSON.parse(readFileSync(path.join(root, "package.json"))).version,
      "0.1.0",
    );
  }));
test("stages every target with architecture-specific asset names", () =>
  fixture((root) => {
    const out = path.join(root, "out");
    for (const t of targets) {
      const files =
        t.platform === "windows"
          ? ["CodeCal_1.2.3_x64-setup.exe"]
          : t.platform === "macos"
            ? ["CodeCal_1.2.3.dmg"]
            : ["CodeCal_1.2.3.deb", "CodeCal_1.2.3.AppImage"];
      const source = path.join(root, t.target);
      mkdirSync(source, { recursive: true });
      const paths = files.map((name) => {
        const file = path.join(source, name);
        writeFileSync(file, "synthetic package");
        return file;
      });
      collectAssets({
        version: "1.2.3",
        target: t.target,
        artifactPaths: paths,
        outputDirectory: out,
      });
    }
    assert.equal(readdirSync(out).length, 8);
    const sums = verifyReleaseAssets(out, "1.2.3");
    assert.equal(sums.split("\n").filter(Boolean).length, 8);
    assert.match(sums, /^[a-f0-9]{64} {2}CodeCal_1.2.3_/m);
    writeFileSync(path.join(out, "unexpected.exe"), "unexpected package");
    assert.throws(() => verifyReleaseAssets(out, "1.2.3"));
  }));
test("rejects incomplete duplicate or empty target bundles", () =>
  fixture((root) => {
    const source = path.join(root, "CodeCal_1.2.3.AppImage");
    writeFileSync(source, "synthetic");
    const args = {
      version: "1.2.3",
      target: "aarch64-unknown-linux-gnu",
      artifactPaths: [source],
      outputDirectory: path.join(root, "out"),
    };
    assert.throws(() => collectAssets(args));
    const empty = path.join(root, "CodeCal_1.2.3-empty-setup.exe");
    writeFileSync(empty, "");
    assert.throws(() =>
      collectAssets({
        ...args,
        target: "x86_64-pc-windows-msvc",
        artifactPaths: [empty],
      }),
    );
    const exe = path.join(root, "CodeCal_1.2.3-setup.exe");
    writeFileSync(exe, "synthetic");
    assert.throws(() =>
      collectAssets({
        ...args,
        target: "x86_64-pc-windows-msvc",
        artifactPaths: [exe, exe],
      }),
    );
  }));
test("refuses publishing partial release files", () =>
  fixture((root) => {
    writeFileSync(
      path.join(root, "CodeCal_1.2.3_windows_x64-setup.exe"),
      "synthetic",
    );
    assert.throws(() => verifyReleaseAssets(root, "1.2.3"));
  }));

test("writes safe single-line GitHub matrix and version outputs", () =>
  fixture((root) => {
    const output = path.join(root, "github-output");
    execFileSync(
      process.execPath,
      [fileURLToPath(new URL("./release.mjs", import.meta.url)), "plan"],
      {
        env: {
          ...process.env,
          RELEASE_TAG: "v1.2.3-rc.1",
          GITHUB_OUTPUT: output,
        },
      },
    );
    const lines = readFileSync(output, "utf-8").trim().split("\n");
    assert.ok(lines.includes("version=1.2.3-rc.1"));
    assert.ok(lines.includes("prerelease=true"));
    const matrix = JSON.parse(
      lines.find((line) => line.startsWith("matrix=")).slice(7),
    );
    assert.deepEqual(matrix.include, targets);
  }));
