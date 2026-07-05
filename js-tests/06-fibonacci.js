function heavyProcess(iterations) {
  // 1. Deep recursion to fill QuickJS's call stack
  function computeFibonacci(n) {
    if (n <= 1) return n;
    return computeFibonacci(n - 1) + computeFibonacci(n - 2);
  }

  let finalResult = 0;

  for (let i = 0; i < iterations; i++) {
    // 2. Memory Churn (Massive creation and destruction of objects)
    // Since QuickJS uses reference counting + cycle GC, this slows it down significantly.
    let container = {};
    for (let j = 0; j < 100; j++) {
      container["key_" + j] = {
        id: j,
        data: Math.random(),
        // Circular reference to force the cycle detector algorithm
        self: container,
      };
    }

    // 3. Extreme polymorphism inside an array
    // Engines with JIT optimize arrays with the same "shape".
    // QuickJS must look up the property in the dictionary every single time.
    let polymorphicArray = [
      { type: "A", value: i },
      { type: "B", text: "hello", value: i * 2 },
      { type: "C", flag: true, compute: () => i },
    ];

    let index = i % 3;
    if (typeof polymorphicArray[index].compute === "function") {
      finalResult += polymorphicArray[index].compute();
    } else {
      finalResult += polymorphicArray[index].value || 0;
    }

    // 4. Mix with a non-optimizable recursive mathematical operation
    if (i % 500 === 0) {
      finalResult += computeFibonacci(20);
    }
  }

  return finalResult;
}

// Execution and benchmarking using performance.now()
const start = performance.now();

const result = heavyProcess(5000);

const end = performance.now();
const duration = end - start;

console.log("RESULT", result);
console.log("TIME_MS", duration.toFixed(4));
