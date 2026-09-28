#!/usr/bin/env node
// Rebuilds the iPad web app and the Vision-Pro desktop app (via `tauri build`)
// and publishes the resulting portable exe + NSIS installer into installer/,
// alongside an updated README with fresh SHA-256 fingerprints.
//
// Usage: npm run package:installer

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, copyFileSync, existsSync, readdirSync, unlinkSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const desktopDir = path.join(repoRoot, "apps", "desktop");
const tauriConfPath = path.join(desktopDir, "src-tauri", "tauri.conf.json");
const releaseDir = path.join(desktopDir, "src-tauri", "target", "release");
const installerDir = path.join(repoRoot, "installer");

function sha256(filePath) {
  return createHash("sha256").update(readFileSync(filePath)).digest("hex").toUpperCase();
}

function run(cmd, args, cwd) {
  console.log(`\n> ${cmd} ${args.join(" ")}  (cwd: ${cwd})`);
  const isWindows = process.platform === "win32";
  // On Windows, pass a single pre-joined command string with shell:true
  // rather than an args array, to avoid Node's shell-array-escaping warning.
  const result = isWindows
    ? spawnSync([cmd, ...args].join(" "), { cwd, stdio: "inherit", shell: true })
    : spawnSync(cmd, args, { cwd, stdio: "inherit" });
  if (result.status !== 0) {
    console.error(`\nCommand failed: ${cmd} ${args.join(" ")} (exit ${result.status})`);
    process.exit(result.status ?? 1);
  }
}

const conf = JSON.parse(readFileSync(tauriConfPath, "utf8"));
const version = conf.version;
const productName = conf.productName ?? "Vision-Pro";

// The iPad web app is embedded in the agent at compile time: build it first so the installer
// never ships a stale copy. `tauri build` then builds the desktop UI and the Rust app.
run("npm", ["run", "build", "-w", "@vision-pro/ipad"], repoRoot);
run("npx", ["tauri", "build"], desktopDir);

const portableExe = path.join(releaseDir, "vision-pro.exe");
const setupExeName = `${productName}_${version}_x64-setup.exe`;
const setupExe = path.join(releaseDir, "bundle", "nsis", setupExeName);

if (!existsSync(portableExe)) {
  console.error(`Expected portable exe not found: ${portableExe}`);
  process.exit(1);
}
if (!existsSync(setupExe)) {
  console.error(`Expected NSIS installer not found: ${setupExe}`);
  process.exit(1);
}

if (!existsSync(installerDir)) {
  console.error(`installer/ directory not found at ${installerDir}`);
  process.exit(1);
}

const destPortable = path.join(installerDir, "vision-pro.exe");
const destSetup = path.join(installerDir, setupExeName);

// Remove stale setup exes from previous versions so installer/ only ever
// holds the current build.
for (const entry of readdirSync(installerDir)) {
  if (/_x64-setup\.exe$/i.test(entry) && entry !== setupExeName) {
    unlinkSync(path.join(installerDir, entry));
    console.log(`Removed stale installer: ${entry}`);
  }
}

copyFileSync(portableExe, destPortable);
copyFileSync(setupExe, destSetup);

const portableHash = sha256(destPortable);
const setupHash = sha256(destSetup);

const readmePath = path.join(installerDir, "README.md");
const readme = `# Vision-Pro — installation locale

- Lancez \`${setupExeName}\` pour installer ou mettre à jour Vision-Pro.
- \`vision-pro.exe\` est le binaire portable correspondant à cette même version. Préférez l’installateur pour une installation normale.

Les réglages et appareils restent dans \`%APPDATA%\\\\Vision-Pro\`; ne supprimez pas ce dossier pour une simple mise à jour.

## Empreintes SHA-256

\`\`\`text
${setupExeName}  ${setupHash}
vision-pro.exe                  ${portableHash}
\`\`\`
`;

writeFileSync(readmePath, readme, "utf8");

console.log(`\nPublished to installer/:`);
console.log(`  ${destSetup}`);
console.log(`    sha256: ${setupHash}`);
console.log(`  ${destPortable}`);
console.log(`    sha256: ${portableHash}`);
console.log(`\nREADME updated: ${readmePath}`);
