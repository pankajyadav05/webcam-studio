// Builds the updater feed (latest.json) that installed apps read.
// Usage: node scripts/release/feed.mjs <version> <base-url> <archive.sig> <notes-file> > latest.json
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SEMVER = /^\d+\.\d+\.\d+$/;

export function names(version) {
  if (!SEMVER.test(version)) throw new Error(`Not a release version: ${version}`);
  return {
    dmg: `Webcam-Studio_${version}_aarch64.dmg`,
    archive: `Webcam-Studio_${version}_aarch64.app.tar.gz`,
    dir: `releases/${version}`,
  };
}

export function buildFeed({ version, base, signature, notes, pubDate }) {
  const url = new URL(base);
  if (url.protocol !== "https:") throw new Error("The release base must use HTTPS");
  const sig = signature.trim();
  if (!sig) throw new Error("Missing updater signature");
  if (Number.isNaN(Date.parse(pubDate))) throw new Error("Invalid publication date");
  const n = names(version);
  return {
    version,
    notes: notes.trim(),
    pub_date: new Date(pubDate).toISOString(),
    platforms: {
      "darwin-aarch64": { url: `${base.replace(/\/+$/, "")}/${n.dir}/${n.archive}`, signature: sig },
    },
  };
}

// True when `next` is a newer release than `current` (both x.y.z).
export function isNewer(next, current) {
  const a = next.split(".").map(Number);
  const b = current.split(".").map(Number);
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] > b[i];
  return false;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [version, base, sigFile, notesFile] = process.argv.slice(2);
  const feed = buildFeed({
    version,
    base,
    signature: readFileSync(sigFile, "utf8"),
    notes: notesFile ? readFileSync(notesFile, "utf8") : "",
    pubDate: new Date().toISOString(),
  });
  process.stdout.write(`${JSON.stringify(feed, null, 2)}\n`);
}
