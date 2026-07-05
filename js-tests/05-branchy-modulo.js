// Branchy loop with modulo. This stresses branches, remainder, comparisons, and accumulator updates.

function nowMs() {
  return Date.now();
}

function run() {
  var sum = 0;
  for (var i = 1; i <= 25000000; i++) {
    if ((i % 3) === 0) {
      sum = sum + (i % 97);
    } else if ((i % 5) === 0) {
      sum = sum - (i % 89);
    } else {
      sum = sum + ((i & 15) - (i % 7));
    }
  }
  return sum;
}

var t0 = nowMs();
var result = run();
var t1 = nowMs();

console.log("RESULT", result);
console.log("TIME_MS", t1 - t0);
