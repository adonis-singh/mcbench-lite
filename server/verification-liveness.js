'use strict';
// The Minecraft process is the live server's child: it fails the verification
// once it has exited or been stopped. Simulation time is uncapped.
function runtimeLivenessCheck(live) {
  return function check() {
    const server = live.server;
    if (!server || server.exitCode !== null || server.signalCode !== null) {
      throw new Error('Minecraft verification runtime exited or became unavailable before producing its result; this is an infrastructure failure, not a circuit verdict');
    }
  };
}
module.exports = { runtimeLivenessCheck };
