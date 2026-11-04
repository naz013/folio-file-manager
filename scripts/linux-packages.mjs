// Builds the Linux packages (.deb and .rpm) in Docker, the same ones the CI build-linux job makes,
// and checks them the way the package-smoke job does. The frontend must be built first
// (`npm run build`): the container has no Node.
// Usage: npm run linux:packages
import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";

const image = "folio-linux";
const root = resolve(import.meta.dirname, "..");

function run(args) {
  const result = spawnSync("docker", args, { stdio: "inherit", cwd: root });
  if (result.error) {
    console.error(`Could not run docker: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run(["build", "-t", image, "-f", "docker/linux.Dockerfile", "docker"]);
mkdirSync(resolve(root, "build"), { recursive: true });

// The packages go to the volume's bundle folder, then are copied to ./dist-linux for a look.
const command = [
  "command -v cargo-tauri >/dev/null || cargo install tauri-cli --version '^2' --locked",
  // The frontend is already built, so the config's own build command is switched off.
  `cargo tauri build --bundles deb,rpm --config '{"build":{"beforeBuildCommand":""}}'`,
  "mkdir -p /work/dist-linux",
  "cp -v target/release/bundle/deb/*.deb target/release/bundle/rpm/*.rpm /work/dist-linux/",
].join(" && ");

run([
  "run",
  "--rm",
  "-v",
  `${root}:/work`,
  "-v",
  "folio-linux-release:/work/src-tauri/target",
  "-v",
  "folio-linux-cargo:/usr/local/cargo/registry",
  "-v",
  "folio-linux-bin:/usr/local/cargo/bin",
  "-w",
  "/work/src-tauri",
  image,
  "bash",
  "-c",
  command,
]);
