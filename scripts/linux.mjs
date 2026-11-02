// Runs the Linux Rust checks in Docker, the same ones as the rust-linux CI job.
// Usage: npm run linux -- <fmt | clippy | test | all | shell> [extra cargo test args]
import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";

const image = "folio-linux";
const root = resolve(import.meta.dirname, "..");
const manifest = "--manifest-path src-tauri/Cargo.toml";

const steps = {
  fmt: `cargo fmt ${manifest} -- --check`,
  clippy: `cargo clippy ${manifest} --all-targets -- -D warnings`,
  test: `cargo test ${manifest}`,
};
steps.all = Object.values(steps).join(" && ");

const [task = "all", ...rest] = process.argv.slice(2);
if (task !== "shell" && !(task in steps)) {
  console.error("Usage: npm run linux -- <fmt | clippy | test | all | shell>");
  process.exit(2);
}

function run(args) {
  const result = spawnSync("docker", args, { stdio: "inherit", cwd: root });
  if (result.error) {
    console.error(`Could not run docker: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run(["build", "-t", image, "-f", "docker/linux.Dockerfile", "docker"]);

// tauri::generate_context! needs the frontend build directory to exist.
mkdirSync(resolve(root, "build"), { recursive: true });

const command = task === "shell" ? "bash" : [steps[task], ...rest].join(" ");
run([
  "run",
  "--rm",
  "-it",
  "-v",
  `${root}:/work`,
  // Build output and the crate cache live in volumes, not in the Windows checkout: this keeps
  // Linux artifacts out of src-tauri/target and avoids slow writes across the bind mount.
  "-v",
  "folio-linux-target:/work/src-tauri/target",
  "-v",
  "folio-linux-cargo:/usr/local/cargo/registry",
  image,
  "bash",
  "-c",
  command,
]);
