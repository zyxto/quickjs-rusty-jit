// Bitwise-heavy integer loop. Stresses shifts, xor, and int32 wrapping semantics.

function run() {
  var hash = 0x12345678;
  for (var i = 0; i < 30000000; i++) {
    hash = (hash + ((i & 255) ^ (i >>> 3))) | 0;
    hash = (hash ^ (hash << 5) ^ (hash >>> 2)) | 0;
  }
  return hash;
}

var t0 = performance.now();
var result = run();
var t1 = performance.now();

console.log("RESULT", result);
console.log("TIME_MS", (t1 - t0).toFixed(4));
