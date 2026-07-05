// Bitwise-heavy integer loop. Stresses shifts, xor, and int32 wrapping semantics.

function nowMs() {
  return Date.now();
}

function run() {
  var hash = 0x12345678;
  for (var i = 0; i < 30000000; i++) {
    hash = (hash + ((i & 255) ^ (i >>> 3))) | 0;
    hash = (hash ^ (hash << 5) ^ (hash >>> 2)) | 0;
  }
  return hash;
}

var t0 = nowMs();
var result = run();
var t1 = nowMs();

console.log("RESULT", result);
console.log("TIME_MS", t1 - t0);
