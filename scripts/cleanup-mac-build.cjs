const fs = require('node:fs');
const path = require('node:path');
const { version, build } = require('../package.json');

const releaseDir = path.resolve(__dirname, '..', 'release');
const dmg = path.join(releaseDir, `${build.productName}-${version}-arm64.dmg`);
if (!fs.existsSync(dmg)) {
  throw new Error(`DMG was not created: ${dmg}`);
}

// electron-builder's unpacked app is indexed by Spotlight as a second install.
fs.rmSync(path.join(releaseDir, 'mac-arm64', `${build.productName}.app`), {
  recursive: true,
  force: true,
});
