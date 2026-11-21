import process from "node:process";
import console from "node:console";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import {
  readFileSync,
  writeFileSync,
  appendFileSync,
  mkdirSync,
  readdirSync,
  lstatSync,
  copyFileSync,
  constants,
} from "node:fs";
const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
export const targets = JSON.parse(
  readFileSync(
    path.join(scriptDirectory, "../.github/release-targets.json"),
    "utf-8",
  ).replace(/^\uFEFF/, ""),
);
export function parseReleaseTag(tag) {
  const match =
    typeof tag === "string" &&
    /^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/.exec(
      tag,
    );
  if (
    !match ||
    match[0] !== tag ||
    match[4]?.split(".").some((part) => /^0[0-9]+$/.test(part))
  )
    throw new Error(
      "Release tag must be vMAJOR.MINOR.PATCH, optionally with a SemVer prerelease/build suffix.",
    );
  return { version: tag.slice(1), prerelease: Boolean(match[4]) };
}
function replaceOne(text, pattern, version, label) {
  if ((text.match(pattern) ?? []).length !== 1)
    throw new Error(`Cannot locate the application version in ${label}`);
  return text.replace(
    pattern,
    (_match, prefix, suffix) => `${prefix}${version}${suffix}`,
  );
}
export function applyReleaseVersion(root, tag) {
  const { version } = parseReleaseTag(tag);
  const names = [
    "package.json",
    "package-lock.json",
    "src-tauri/tauri.conf.json",
    "src-tauri/Cargo.toml",
    "src-tauri/Cargo.lock",
  ];
  const changes = names.map((name) => {
    const file = path.join(root, name);
    const content = readFileSync(file, "utf-8");
    if (name.endsWith(".json")) {
      const value = JSON.parse(content.replace(/^\uFEFF/, ""));
      value.version = version;
      if (name === "package-lock.json") {
        if (!value.packages?.[""])
          throw new Error("package-lock.json has no root package");
        value.packages[""].version = version;
      }
      return [file, JSON.stringify(value, null, 2) + "\n"];
    }
    const pattern = name.endsWith("Cargo.toml")
      ? /(\[package\][\s\S]*?^version\s*=\s*")[^"]+("[^\r\n]*)/gm
      : /(\[\[package\]\]\r?\nname = "codecal"\r?\nversion = ")[^"]+("[^\r\n]*)/g;
    return [file, replaceOne(content, pattern, version, name)];
  });
  // Validate all input files before changing any of them.
  for (const [file, content] of changes) writeFileSync(file, content);
  return version;
}
function suffixes(platform) {
  if (platform === "windows") return ["-setup.exe"];
  if (platform === "macos") return [".dmg"];
  if (platform === "linux") return [".deb", ".AppImage"];
  throw new Error(`Unsupported platform: ${platform}`);
}
function assetName(version, target, suffix) {
  return `CodeCal_${version}_${target.platform}_${target.arch}${suffix}`;
}
function nonEmptyFile(file) {
  const stat = lstatSync(file);
  if (!stat.isFile() || stat.size === 0)
    throw new Error(`Missing or empty release file: ${file}`);
}
export function collectAssets({
  version,
  target,
  artifactPaths,
  outputDirectory,
}) {
  parseReleaseTag(`v${version}`);
  const spec = targets.find((t) => t.target === target);
  if (!spec) throw new Error(`Unsupported release target: ${target}`);
  if (
    !Array.isArray(artifactPaths) ||
    artifactPaths.some((p) => typeof p !== "string")
  )
    throw new Error("artifactPaths must be a JSON array of paths");
  const files = suffixes(spec.platform).map((suffix) => {
    const matches = artifactPaths.filter((file) => file.endsWith(suffix));
    if (matches.length !== 1)
      throw new Error(
        `Expected one ${suffix} artifact for ${target}, got ${matches.length}`,
      );
    const source = matches[0];
    nonEmptyFile(source);
    if (!path.basename(source).includes(version))
      throw new Error(`Artifact version does not match ${version}: ${source}`);
    return [
      source,
      path.join(outputDirectory, assetName(version, spec, suffix)),
    ];
  });
  mkdirSync(outputDirectory, { recursive: true });
  for (const [source, destination] of files)
    copyFileSync(source, destination, constants.COPYFILE_EXCL);
  return files.map(([, destination]) => destination);
}
export function verifyReleaseAssets(directory, version) {
  parseReleaseTag(`v${version}`);
  const expected = targets
    .flatMap((t) =>
      suffixes(t.platform).map((suffix) => assetName(version, t, suffix)),
    )
    .sort();
  const actual = readdirSync(directory)
    .filter((name) => name !== "SHA256SUMS")
    .sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected))
    throw new Error(
      `Release artifacts are incomplete or unexpected. Expected: ${expected.join(", ")}. Found: ${actual.join(", ")}`,
    );
  return expected
    .map((name) => {
      const file = path.join(directory, name);
      nonEmptyFile(file);
      return `${createHash("sha256").update(readFileSync(file)).digest("hex")}  ${name}\n`;
    })
    .join("");
}
function main() {
  const command = process.argv[2];
  const directory = path.resolve(process.env.RELEASE_OUTPUT_DIR ?? ".release");
  if (command === "plan") {
    const release = parseReleaseTag(process.env.RELEASE_TAG);
    const output = { ...release, matrix: JSON.stringify({ include: targets }) };
    if (process.env.GITHUB_OUTPUT)
      appendFileSync(
        process.env.GITHUB_OUTPUT,
        Object.entries(output)
          .map(([key, value]) => `${key}=${value}\n`)
          .join(""),
      );
    else
      console.log(JSON.stringify({ ...release, matrix: { include: targets } }));
  } else if (command === "version") {
    console.log(
      `Release version: ${applyReleaseVersion(process.cwd(), process.env.RELEASE_TAG)}`,
    );
  } else if (command === "collect") {
    const { version } = parseReleaseTag(process.env.RELEASE_TAG);
    const files = collectAssets({
      version,
      target: process.env.RELEASE_TARGET,
      artifactPaths: JSON.parse(process.env.TAURI_ARTIFACT_PATHS ?? "[]"),
      outputDirectory: directory,
    });
    console.log(files.join("\n"));
  } else if (command === "verify") {
    const { version } = parseReleaseTag(process.env.RELEASE_TAG);
    const sums = verifyReleaseAssets(directory, version);
    writeFileSync(path.join(directory, "SHA256SUMS"), sums);
    console.log(sums);
  } else
    throw new Error(
      "Usage: node scripts/release.mjs <plan|version|collect|verify>",
    );
}
if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    main();
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
