// Loaded by server.js from beside it. Starts the stand's program by its
// path, never through a shell, on the standard streams this runtime was
// given, with the environment it was given, and ends as that program ends.
'use strict';
const { spawn } = require('child_process');

function serve(program, args) {
  if (!program) {
    process.stderr.write('server.js: the stand program is its first argument\n');
    process.exit(2);
  }
  const child = spawn(program, args, { stdio: 'inherit', shell: false, windowsHide: true });
  child.on('error', () => process.exit(4));
  child.on('exit', (code) => process.exit(code === null ? 5 : code));
}

module.exports = { serve };
