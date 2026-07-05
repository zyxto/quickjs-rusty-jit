// Hot counted loop with a constant accumulator update.
// This is intentionally simple: it should expose interpreter overhead and any loop-folding/JIT path.

function nowMs() {
  return Date.now();
}

function run() {
  var count = 0;
  for (var i = 0; i < 150000000; i++) {
    count = count + 6;
  }
  return count;
}

var t0 = nowMs();
var result = run();
var t1 = nowMs();

console.log("RESULT", result);
console.log("TIME_MS", t1 - t0);
