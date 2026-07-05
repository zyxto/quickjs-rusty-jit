# QuickJS JIT Compiler Project Resume Prompt

Este documento resume el estado actual del proyecto `quickjs-assembler` y deja el próximo paso técnico recomendado para continuar implementando optimizaciones tipo LuaJIT sobre QuickJS.

---

## 🎯 Objetivo del proyecto

Estamos construyendo un **JIT nativo para QuickJS** que traduce bytecode stack-based de QuickJS a un bytecode register-based propio y luego a instrucciones x86_64 ejecutables.

La meta es lograr que loops numéricos y lógica matemática chica corran a velocidades comparables o mejores que runtimes modernos como Bun/V8/JSC, especialmente en micro-hotspots donde el costo del intérprete de QuickJS domina.

El objetivo práctico actual es acelerar patrones como:

- `for (var i = 0; i < N; i++) count = count + C`
- `for (var i = 0; i < N; i++) count = count + i`
- variantes con `i += step`, límites constantes y acumuladores simples
- próximos patrones: `count += i * C`, `count += i % C`, `count += i & mask`, `count = count * C + i`, etc.

---

## 🧠 Arquitectura actual

### 1. Integración con QuickJS

El hook está integrado en `quickjs.c`, dentro de `JS_CallInternal`, llamando a:

- `js_register_interpreter(...)`

Archivo Rust principal:

- `libquickjs-sys/src/register_interpreter.rs`

Flujo actual:

1. QuickJS entra a `JS_CallInternal` para ejecutar una función JS.
2. Antes del intérprete C tradicional, llama a `js_register_interpreter`.
3. `js_register_interpreter` intenta traducir el bytecode QuickJS a register-bytecode.
4. Si puede, intenta compilar ese register-bytecode a x86_64 nativo.
5. Si el código nativo retorna un valor distinto de `JS_UNINITIALIZED`, QuickJS usa ese resultado.
6. Si falla o baila, vuelve al intérprete C normal.

### 2. Compilador stack → register

Archivo:

- `libquickjs-sys/src/compiler.rs`

Responsabilidades:

- Leer bytecode QuickJS stack-based.
- Simular altura de stack.
- Convertir operaciones stack-based a `RegInstruction`.
- Resolver saltos a índices de instrucciones register-based.

Puntos importantes ya descubiertos:

- Los offsets de salto de QuickJS (`goto8`, `goto16`, `if_false8`, etc.) se calculan relativos al **byte de offset**, es decir:
  - `target_pc = pc + 1 + offset`
- Esto es crítico. Si se calcula relativo al final de la instrucción, aparecen loops infinitos, saltos a límites inválidos y stack underflows.

Bytecodes QuickJS ya soportados relevantes:

- constantes: `push_i32`, `push_i8`, `push_i16`, `push_minus1`, `push_0..push_7`, `push_const`, `push_const8`
- locales: `get_loc`, `get_loc8`, `get_loc0..get_loc3`, `get_loc0_loc1`
- stores: `put_loc`, `put_loc8`, `put_loc0..put_loc3`
- set/local preserving stack: `set_loc`, `set_loc8`, `set_loc0..set_loc3`
- aritmética local: `add_loc`, `inc_loc`, `dec_loc`
- aritmética general: `add`
- comparaciones: `lt`, `lte`, `gt`, `gte`
- control flow: `if_false`, `if_true`, `if_false8`, `if_true8`, `goto`, `goto8`, `goto16`
- return: `return`, `return_undef`

Register opcode agregado importante:

- `OP_REG_SET_LOC`: copia un registro virtual a un local sin consumir el valor stack/register original.

### 3. JIT x86_64

Archivo:

- `libquickjs-sys/src/jit.rs`

Responsabilidades:

- Mapear `RegInstruction` a bytes de máquina x86_64 System V ABI.
- Generar código executable con `mmap(PROT_READ | PROT_WRITE | PROT_EXEC)`.
- Emitir checks de tags (`JS_TAG_INT`, `JS_TAG_BOOL`) y bailouts.
- Emitir retorno como `JSValue` usando registros ABI.

Representación relevante de `JSValue` sin NaN boxing en este build:

- `JSValue` es struct de 16 bytes:
  - `u`: payload (`int32`, `float64`, pointer, etc.)
  - `tag`: `i64`
- Retorno de `JSValue` en x86_64 System V:
  - payload en `rax`
  - tag en `rdx`

Tags relevantes:

- `JS_TAG_INT = 0`
- `JS_TAG_BOOL = 1`
- `JS_TAG_NULL = 2`
- `JS_TAG_UNDEFINED = 3`
- `JS_TAG_UNINITIALIZED = 4`
- `JS_TAG_FLOAT64 = 8`

### 4. Fallback VM register-based

Archivo:

- `libquickjs-sys/src/register_interpreter.rs`

Responsabilidades:

- Ejecutar el register-bytecode cuando no se puede generar nativo.
- Retornar `JS_UNINITIALIZED` para indicar fallback al intérprete C.
- Manejar `js_dup` / `js_free` para valores ref-counted cuando corresponde.

### 5. Caché nativa actual

En `register_interpreter.rs` existe una caché simple de código nativo:

- keyed por hash del bytecode + longitud
- guarda dirección de entrada del buffer JIT
- límite actual aproximado: 4096 entradas
- si un stub cacheado baila (`JS_UNINITIALIZED`), se elimina del caché

Nota importante: actualmente los buffers JIT cacheados se filtran intencionalmente con `std::mem::forget` para preservar ejecutabilidad/lifetime. Esto es aceptable como prototipo, pero en una fase futura conviene diseñar ownership/lifetime real asociado al ciclo de vida de `JSFunctionBytecode`.

---

## 🚀 Estado de performance actual

### Caso `count += constant`

Patrón probado:

- `for (var i = 0; i < 100000000; i++) count = count + 6`

Resultado esperado:

- `600000000`

Estado actual:

- El recognizer de loop contado detecta el patrón.
- En vez de emitir loop nativo, calcula la forma cerrada y retorna el resultado como constante JS nativa.
- Tiempo observado localmente: `~0ms` con `Date.now`, previamente `~0.018ms` con medición de alta resolución.
- Esto supera ampliamente el caso Bun observado por el usuario (`~28ms`) para ese escenario.

### Caso `count += i`

Patrón probado:

- `for (var i = 0; i < 1000000; i++) count = count + i`

Resultado esperado:

- `499999500000`

Estado actual:

- Antes: corría nativo hasta overflow `i32`, bailaba alrededor de `65536`, y terminaba en intérprete C.
- Ahora: el recognizer calcula la suma cerrada:
  - `n * i0 + step * n * (n - 1) / 2`
- Si el resultado entra en `i32`, retorna `JS_TAG_INT`.
- Si excede `i32` pero entra en `Number.MAX_SAFE_INTEGER`, retorna `JS_TAG_FLOAT64`.
- Tiempo observado localmente: `~0ms` con `Date.now`.

---

## ✅ Lo que ya está implementado

1. Resolución correcta de jumps QuickJS con `pc + 1 + offset`.
2. Traducción stack → register de loops básicos.
3. JIT x86_64 con checks de tags y bailouts.
4. Comparaciones enteras y branches nativos.
5. Folding de loops contados con acumulador constante.
6. Folding de loops contados con acumulador `count += i`.
7. Retorno nativo de `JS_TAG_INT` y `JS_TAG_FLOAT64`.
8. Soporte de short opcodes frecuentes de QuickJS-ng.
9. Caché simple de stubs nativos por hash de bytecode.
10. Debug opcional con env var:
    - `QJS_JIT_TRACE=1`

---

## 📋 Próximo paso recomendado

El próximo paso útil no es sólo seguir agregando opcodes; hay que implementar una fase de optimización de loops con **registro CPU persistente** y más expresiones aritméticas reconocibles.

La meta es cubrir escenarios donde no podamos, o no queramos, plegar todo a constante, pero aun así queramos correr el loop en CPU registers sin tocar memoria en cada iteración.

---

# Próxima fase: Optimized Counted Loop Backend

## Objetivo

Implementar un backend especializado para loops contados que mantenga variables calientes en registros CPU durante toda la iteración.

Ejemplo objetivo:

```text
var count = 0;
for (var i = 0; i < N; i++) {
    count = count + expr(i);
}
return count;
```

Donde `expr(i)` pueda ser:

- constante: `C`
- inducción directa: `i`
- affine: `i * C + K`
- bitwise barato: `i & mask`
- módulo por constante pequeña: `i % C`
- combinaciones simples sin side effects

## Por qué hace falta

El folding a constante es brutalmente rápido, pero sólo aplica cuando:

- límites son constantes o inferibles
- no hay side effects
- el resultado es seguro de representar igual que JS
- podemos probar la forma cerrada

Para escenarios más generales, por ejemplo:

- límite pasado como argumento
- acumuladores iniciales variables
- loops llamados muchas veces con distintos `N`
- expresiones más complejas
- resultados que no conviene cerrar algebraicamente

necesitamos emitir un loop nativo real, pero optimizado.

---

## Diseño recomendado

### 1. Agregar IR de análisis de loop

Antes de emitir x86_64 directo, crear una estructura interna tipo:

```text
CountedLoop {
    loop_start,
    backedge,
    exit,
    induction: InductionVar,
    accumulators: Vec<Accumulator>,
    guards: Vec<Guard>,
}
```

Ejemplo conceptual:

```text
InductionVar {
    loc: u8,
    init: ValueSource,
    limit: ValueSource,
    step: i32,
    cmp: Lt/Lte/Gt/Gte,
}

Accumulator {
    loc: u8,
    init: ValueSource,
    update: Expr,
}

Expr = Const(i32)
     | Induction
     | Add(Box<Expr>, Box<Expr>)
     | MulConst(Box<Expr>, i32)
     | BitAndConst(Box<Expr>, i32)
     | ModConst(Box<Expr>, i32)
```

No hace falta implementar todo de una vez. Primero conviene modelar:

- `Const`
- `Induction`
- `Add`
- `MulConst`

### 2. Separar recognizer de emitter

Actualmente `compile_constant_counted_loop` reconoce y emite en el mismo lugar.

Conviene dividir:

1. `analyze_counted_loop(bytecode) -> Option<CountedLoop>`
2. `try_fold_counted_loop(loop) -> Option<Vec<u8>>`
3. `compile_counted_loop_native(loop) -> Option<Vec<u8>>`

Esto permite:

- mantener folding para casos cerrados
- emitir loop nativo para casos variables
- reutilizar análisis para futuras optimizaciones

### 3. Backend nativo con registros persistentes

En vez de hacer cada iteración con loads/stores desde `regs[i]` y `var_buf[i]`, usar registros CPU:

Propuesta inicial de asignación:

- `r8`: `var_buf` pointer, ya usado actualmente
- `r9`: `cpool` pointer, ya usado actualmente
- `rbx`: accumulator principal `count`
- `r12`: induction `i`
- `r13`: limit
- `r14`: step o temporal
- `r15`: segundo acumulador/temporal

Importante:

- `rbx`, `r12`-`r15` son callee-saved en System V ABI.
- Si se usan, el JIT debe preservarlos:
  - prologue: `push rbx`, `push r12`, ...
  - epilogue: `pop ...` en orden inverso
- Si sólo se usan caller-saved (`rax`, `rcx`, `r10`, `r11`) no hace falta preservar, pero hay menos registros y se complica para loops más ricos.

### 4. Guards antes del loop

Antes de correr el loop optimizado, emitir guards una sola vez:

- locales involucrados tienen `JS_TAG_INT`, o `JS_TAG_FLOAT64` si el modo lo permite
- límite es int si viene de local/arg/const
- step no es cero
- no hay overflow si se elige modo int32 estricto

Si un guard falla:

- retornar `JS_UNINITIALIZED`
- QuickJS cae al intérprete C

### 5. Modos numéricos

Hay dos modos útiles:

#### Modo A: Int32 checked

- operar con `i32`
- después de `add`, `sub`, `mul`, chequear overflow (`jo bailout`)
- si overflow: bailout al intérprete C

Ventaja:

- Semántica simple y compatible con QuickJS para enteros pequeños.

Desventaja:

- `count += i` bailable rápido si supera `i32`.

#### Modo B: Float64 accumulation

- convertir accumulator a double temprano o cuando se predice overflow
- usar SSE2 (`xmm0`, `xmm1`, etc.)
- retornar `JS_TAG_FLOAT64`

Ventaja:

- Evita bailout para sumas grandes como JS Number.

Desventaja:

- Hay que cuidar exactitud JS:
  - enteros hasta `2^53 - 1` son exactos
  - más allá puede haber diferencias por orden de suma

Recomendación:

- primero implementar modo int32 checked para loop nativo real
- luego modo float64 para acumuladores que se sabe que exceden `i32` pero siguen en rango seguro

---

## Opcodes próximos a implementar

### En `compiler.rs`

Agregar register opcodes para:

- `OP_REG_SUB`
- `OP_REG_MUL`
- `OP_REG_MOD`
- `OP_REG_BIT_AND`
- `OP_REG_BIT_OR`
- `OP_REG_BIT_XOR`
- opcional: `OP_REG_SHL`, `OP_REG_SAR`, `OP_REG_SHR`

QuickJS opcode IDs relevantes desde `quickjs-opcode.h`:

- `mul` aparece antes de `div`, `mod`, `add`, `sub`
- en el enum actual, ya se sabe que:
  - `OP_ADD = 156`
  - por orden QuickJS:
    - `OP_MUL = 153`
    - `OP_DIV = 154`
    - `OP_MOD = 155`
    - `OP_SUB = 157`
    - `OP_SHL = 158`
    - `OP_SAR = 159`
    - `OP_SHR = 160`
    - `OP_AND = 161`
    - `OP_XOR = 162`
    - `OP_OR = 163`

Verificar en `quickjs-opcode.h` antes de hardcodear si cambió la versión.

### En `register_interpreter.rs`

Implementar fallback VM para esos opcodes:

- int32 fast path
- overflow checked en `add/sub/mul`
- para `mod`, respetar comportamiento JS con enteros si ambos son int y divisor no cero
- si no se puede garantizar semántica, retornar `JS_UNINITIALIZED`

### En `jit.rs`

Implementar emisión x86_64:

- `sub`: `sub`, `jo bailout`
- `mul`: `imul`, `jo bailout`
- `and/or/xor`: sin overflow
- `mod`: usar `cdq/idiv` con guards para divisor cero y edge case `INT_MIN / -1`

---

## Cosas a pensar para hacerlo bien

### 1. Correctitud antes que performance

Si hay duda semántica, hacer bailout. El intérprete C de QuickJS es la fuente de verdad.

El JIT debe ser especulativo:

- si reconoce tipos y forma: corre rápido
- si algo no encaja: `JS_UNINITIALIZED`

### 2. Side effects

Sólo plegar loops cuando se pueda probar que no hay side effects.

Seguro:

- locales numéricos
- constantes
- aritmética pura
- comparaciones puras

No seguro:

- property access
- calls
- `valueOf` / coerciones
- objetos
- strings
- BigInt
- arrays
- closures/var refs

### 3. Semántica de JS Number

QuickJS usa int32 como representación optimizada, pero JS Number semánticamente es double.

Para operaciones enteras:

- si ambos operandos son int32 y no overflow: resultado int32
- si overflow: QuickJS suele pasar a float64 para Number

El JIT puede:

- hacer bailout en overflow
- o promover a float64 si puede garantizar equivalencia

### 4. Bailout state

Cuidado: si el código nativo modifica `var_buf` y luego baila, el intérprete C continuará con estado parcialmente modificado.

Para loops nativos especulativos hay dos estrategias:

1. No escribir `var_buf` hasta terminar el loop.
   - mantener `i`, `count` en registros CPU
   - al final escribir locals finales
   - si bailout antes del loop: estado intacto

2. Si puede haber bailout dentro del loop, antes de retornar `JS_UNINITIALIZED` hay que materializar estado exacto.
   - más complejo

Recomendación inicial:

- emitir guards antes del loop
- dentro del loop no emitir bailouts salvo overflow controlado
- si hay posibilidad de overflow, elegir:
  - probar rango antes del loop y evitar overflow
  - o no optimizar ese caso

### 5. Rango y overflow

Para loops contados, muchas veces se puede probar rango antes de emitir:

- número de iteraciones
- máximo/mínimo de `i`
- crecimiento de accumulator

Si el resultado entra en `i32`, usar int32.

Si entra en `Number.MAX_SAFE_INTEGER`, se puede considerar float64 exacto.

Si excede rango seguro, no plegar. Para loop nativo float64, considerar diferencias por orden de operaciones.

### 6. Medición

Mantener benchmarks mínimos:

- `count += 1`
- `count += 6`
- `count += i`
- `count += i * 3`
- `count += i & 7`
- `count += i % 10`
- límite como constante
- límite como argumento
- función llamada muchas veces

Comparar contra:

- QuickJS C interpreter
- Bun
- nuestro `qjs`

---

## Comandos útiles

Build release:

- `cargo build --release --bin qjs`

Tests rápidos:

- `cargo test --lib`

Trace de misses JIT:

- `QJS_JIT_TRACE=1 ./target/release/qjs ./script.js`

Ejecutar benchmark:

- `./target/release/qjs ./scripty.js`

---

## Estado de validación al cierre

Validado localmente:

- `cargo build --release --bin qjs`: OK
- `cargo test --lib`: OK, 7 tests passed

Warnings existentes no relacionados:

- `#[cfg(feature = "log")]` aparece como feature inesperada en `src/console.rs` porque `log` no está declarado como feature en `Cargo.toml`.

---

## Resumen para la próxima sesión

Lo más rentable ahora es convertir el optimizador actual de loops en una mini-pipeline:

1. analizar loop contado
2. construir IR simple de inducción/acumuladores
3. intentar folding algebraico
4. si no se puede folding, emitir loop nativo con `i` y `count` en registros CPU
5. agregar opcodes aritméticos (`sub`, `mul`, `mod`, bitwise)
6. extender recognizers a expresiones simples sobre `i`

El objetivo es pasar de “ganamos benchmarks plegables” a “tenemos un JIT de loops numéricos real”, manteniendo la regla de oro: ante cualquier duda semántica, bailout limpio al intérprete de QuickJS.
