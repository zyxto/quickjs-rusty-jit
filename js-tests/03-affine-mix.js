// Affine arithmetic over the induction variable: sum += i * C + K.
// This is the next useful recognizer target after constant and direct induction sums.


function run() {
  var sum = 0;
  for (var i = 0; i < 20000000; i++) {
    sum = sum + i * 3 + 7;
  }
  return sum;
}

var t0 = performance.now();
var result = run();
var t1 = performance.now();

console.log("RESULT", result);
console.log("TIME_MS", (t1 - t0).toFixed(4));
