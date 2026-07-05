// Sum of the induction variable. This stresses int32 overflow behavior and JS Number promotion.

function run() {
  var sum = 0;
  for (var i = 0; i < 20000000; i++) {
    sum = sum + i;
  }
  return sum;
}

var t0 = performance.now();
var result = run();
var t1 = performance.now();

console.log("RESULT", result);
console.log("TIME_MS", (t1 - t0).toFixed(4));
