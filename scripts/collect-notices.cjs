// Generate an inventory and preserve shipped dependency license texts while the
// dependency source trees are present in the image build stages.
const fs = require('node:fs');
const path = require('node:path');
const [kind, source, destination] = process.argv.slice(2);
fs.mkdirSync(destination, { recursive: true });
const packages = [];
function record(root, metadata, extraLicense) {
  if (!metadata.name || !metadata.version) return;
  const directory = path.join(destination, encodeURIComponent(`${metadata.name}@${metadata.version}`));
  fs.mkdirSync(directory, { recursive: true });
  const files = fs.readdirSync(root).filter(name => /^(license|licence|copying|notice)([._-].*)?$/i.test(name));
  if (extraLicense && !extraLicense.includes('..') && !path.isAbsolute(extraLicense)) files.push(extraLicense);
  const saved = [];
  for (const name of new Set(files)) {
    const input = path.join(root, name);
    if (!fs.existsSync(input) || !fs.statSync(input).isFile()) continue;
    const target = name.replaceAll('/', '_');
    fs.copyFileSync(input, path.join(directory, target));
    saved.push(target);
  }
  packages.push({ ...metadata, licenseFiles: saved.sort() });
}
if (kind === 'rust') {
  for (const registry of fs.readdirSync(source)) {
    const registryRoot = path.join(source, registry);
    for (const crate of fs.readdirSync(registryRoot)) {
      const root = path.join(registryRoot, crate);
      const manifest = path.join(root, 'Cargo.toml');
      if (!fs.existsSync(manifest)) continue;
      const section = fs.readFileSync(manifest, 'utf8').split('[package]')[1]?.split(/\n\[/)[0] || '';
      const value = key => section.match(new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, 'm'))?.[1];
      record(root, { name: value('name'), version: value('version'), license: value('license') || null,
                     repository: value('repository') || null }, value('license-file'));
    }
  }
} else if (kind === 'npm') {
  function packageDir(root) {
    const manifest = path.join(root, 'package.json');
    if (!fs.existsSync(manifest)) return;
    const meta = JSON.parse(fs.readFileSync(manifest, 'utf8'));
    record(root, { name: meta.name, version: meta.version, license: meta.license || null,
                   repository: meta.repository || null, homepage: meta.homepage || null });
  }
  for (const entry of fs.readdirSync(source)) {
    const modules = path.join(source, entry, 'node_modules');
    if (!fs.existsSync(modules)) continue;
    for (const name of fs.readdirSync(modules)) {
      const root = path.join(modules, name);
      if (fs.lstatSync(root).isSymbolicLink()) continue;
      if (name.startsWith('@')) {
        for (const child of fs.readdirSync(root)) {
          const scoped = path.join(root, child);
          if (!fs.lstatSync(scoped).isSymbolicLink()) packageDir(scoped);
        }
      } else packageDir(root);
    }
  }
} else throw new Error('Expected rust or npm');
packages.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`));
fs.writeFileSync(path.join(destination, 'inventory.json'), JSON.stringify(packages, null, 2) + '\n');
