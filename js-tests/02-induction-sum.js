// Sum of the induction variable. This stresses int32 overflow behavior and JS Number promotion.

function nowMs() {
  return Date.now();
}

function run() {
  var sum = 0;
  for (var i = 0; i < 20000000; i++) {
    sum = sum + i;
  }
  return sum;
}

var t0 = nowMs();
var result = run();
var t1 = nowMs();

console.log("RESULT", result);
console.log("TIME_MS", t1 - t0);
