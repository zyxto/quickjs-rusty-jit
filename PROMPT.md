# QuickJS JIT Compiler Project Resume Prompt

This document provides complete context, achievements, and technical details to resume the paired session for implementing advanced LuaJIT-like optimizations in the stack-to-register JIT compiler.

---

## 🎯 Project Goal & Context

We are pair-programming to build a **Native JIT compiler for QuickJS** that translates stack-to-register bytecode into executable x86_64 machine instructions. 

* **Why**: To speed up execution times of loop iterations and mathematical logic in Javascript to resemble high-performance JIT VM engines (like LuaJIT).
* **Target Loop Code**:
  ```javascript
  function run() {
      var count = 0;
      for (var i = 0; i < 1000000; i++) {
          count = count + 1;
      }
      return count;
  }
  run();
  ```
* **Performance Goal**: Execute the 1,000,000 iteration loop in **under 10ms** (compared to ~62ms in standard QuickJS interpreter).

---

## 🚀 Key Discoveries & Status (Session Achievements)

1. **Fixed Jump Resolution Offset Bug**: 
   In QuickJS, all bytecode jump offset targets (e.g. `OP_goto8`, `OP_goto16`, `OP_if_false8`) calculate their destination relative to the **offset byte PC** (which is `opcode_pc + 1`), not the instruction's boundary.
   * **Fix**: Updated all translation logic in [compiler.rs](file:///home/alexis/dev/test/quickjs-assembler/libquickjs-sys/src/compiler.rs) to calculate target PC using:
     `let target_pc = (pc as isize + 1 + offset as isize) as usize;`
   * This successfully resolved the infinite loop behavior and stack underflows, enabling loop jumps to correctly point to the load-variable instructions.

2. **JIT Performance Milestones**:
   * **1M iterations loop**: Runs in **~8.0 ms** natively (7.7x faster than the C interpreter).
   * **10M iterations loop**: Runs in **~88.7 ms** natively (7.0x faster than the C interpreter).
   * **Overflow Detection & Bailouts**: Adding `sum = sum + i` correctly compiles and runs native instructions at CPU speeds until the 65,536th iteration, where the signed `i32` overflow flag is set. The JIT detects this, bails out via check `2` (overflow bailout), and transfers state seamlessly back to the C interpreter to complete the loop, producing the correct sum: `499999500000`.

3. **Opcode Additions**:
   * Added `OP_PUSH_I8` and `OP_PUSH_I16` support to translate small integer load constants correctly.

---

## 🛠️ Current Code Architecture

* **Integration Point**: The entry point is registered as a callback hook in `quickjs.c` inside `js_register_interpreter`.
* **Stack-to-Register Compiler**: [compiler.rs](file:///home/alexis/dev/test/quickjs-assembler/libquickjs-sys/src/compiler.rs) analyzes stack states, allocates stack heights to registers, and generates register bytecode.
* **Native Compiler (x86_64)**: [jit.rs](file:///home/alexis/dev/test/quickjs-assembler/libquickjs-sys/src/jit.rs) maps register bytecode directly into System V ABI compliant x86_64 instructions using a page-allocated memory buffer.
* **Fallback VM**: [register_interpreter.rs](file:///home/alexis/dev/test/quickjs-assembler/libquickjs-sys/src/register_interpreter.rs) handles compilation bails and fallback.

---

## 📋 Next Steps for the Next Session

1. **Implement Register Allocation Optimizations**:
   * Instead of moving variables back and forth between memory registers (`regs[i]`) and CPU registers (`rax`, `rcx`), perform register allocation using CPU registers (`rbx`, `r12`-`r15`) to persist values across iterations.
2. **Support More Opcodes**:
   * Expand compiler support for helper built-ins, object access, or property lookups as required.
3. **Advanced LuaJIT techniques**:
   * Type specialization (e.g. speculative integer arithmetic without checking tags if the input tag can be statically inferred or tracked as an invariant).
