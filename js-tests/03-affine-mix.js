// Affine arithmetic over the induction variable: sum += i * C + K.
// This is the next useful recognizer target after constant and direct induction sums.

function nowMs() {
  return Date.now();
}

function run() {
  var sum = 0;
  for (var i = 0; i < 20000000; i++) {
    sum = sum + i * 3 + 7;
  }
  return sum;
}

var t0 = nowMs();
var result = run();
var t1 = nowMs();

console.log("RESULT", result);
console.log("TIME_MS", t1 - t0);
