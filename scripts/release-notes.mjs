// Writes the notes of the release being built into src/lib/release-notes.json, so the app
// can show "What's new" offline. Run by the Release workflow before the build:
//   node scripts/release-notes.mjs <tag>
// The notes are the body of the draft release for the tag if one has a body (write it
// before pushing the tag to choose the words); otherwise GitHub's generated notes, which
// list merged pull requests. Notes are a nicety: any failure here writes empty notes and
// never stops the build.
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";

const tag = process.argv[2];
const repo = process.env.GITHUB_REPOSITORY;

const gh = (...args) => execFileSync("gh", args, { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });

/** What GitHub generates is a title and a compare link when nothing was merged by pull request. */
export function meaningful(body) {
  return body
    .split(/\r?\n/)
    .filter(
      (l) =>
        l.trim() &&
        !/^\*\*Full Changelog\*\*/i.test(l.trim()) &&
        !/^#+\s*What'?s Changed\s*$/i.test(l.trim()),
    )
    .join("\n")
    .trim();
}

function draftBody() {
  // A draft has no tag yet, so it is found in the list, not by tag.
  const releases = JSON.parse(gh("api", `repos/${repo}/releases`, "--paginate", "--slurp")).flat();
  return releases.find((r) => r.tag_name === tag)?.body ?? "";
}

function generated() {
  return (
    JSON.parse(gh("api", `repos/${repo}/releases/generate-notes`, "-f", `tag_name=${tag}`)).body ??
    ""
  );
}

function notesFromGitHub() {
  if (!repo) throw new Error("GITHUB_REPOSITORY is not set");
  for (const [name, get] of [
    ["the release body", draftBody],
    ["generated notes", generated],
  ]) {
    try {
      const body = get();
      if (meaningful(body)) {
        console.log(`Using ${name}`);
        return body.trim();
      }
    } catch (e) {
      console.warn(`Could not read ${name}: ${e.message}`);
    }
  }
  console.warn("No release notes found; the app will say so.");
  return "";
}

if (
  process.argv[1] &&
  import.meta.url.endsWith(process.argv[1].replaceAll("\\", "/").split("/").pop())
) {
  if (!tag) {
    console.error("usage: node scripts/release-notes.mjs <tag>");
    process.exit(2);
  }
  const version = tag.replace(/^v/, "");
  const notes = notesFromGitHub();
  const file = new URL("../src/lib/release-notes.json", import.meta.url);
  writeFileSync(file, JSON.stringify({ version, notes }, null, 2) + "\n");
  console.log(`Release notes for ${version}: ${notes.length} characters`);
}
