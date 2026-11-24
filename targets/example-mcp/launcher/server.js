// The stand's server as an installed package's entry file, which the
// JavaScript runtime runs: the form most local MCP servers are started in.
// It loads a file beside it, as an installed entry file does, and so runs
// only where it lies. The file it loads starts the stand's own program,
// named by the first argument, on the same standard streams. Neither the
// file beside it nor that program is pinned by the entry file's digest.
'use strict';
const relay = require('./relay.js');
relay.serve(process.argv[2], process.argv.slice(3));
