const { execFileSync } = require('node:child_process');
const path = require('node:path');

module.exports = async ({ appOutDir, packager }) => {
  const app = path.join(appOutDir, `${packager.appInfo.productFilename}.app`);
  for (const binary of ['local_proxy_core', 'local_proxy_wireguard_helper']) {
    execFileSync('/usr/bin/codesign', [
      '--force', '--sign', '-',
      path.join(app, 'Contents', 'Resources', 'bin', binary),
    ], { stdio: 'inherit' });
  }
  execFileSync('/usr/bin/codesign', [
    '--force', '--deep', '--sign', '-', app,
  ], { stdio: 'inherit' });
  execFileSync('/usr/bin/codesign', [
    '--verify', '--deep', '--strict', '--verbose=2', app,
  ], { stdio: 'inherit' });
};
