---
title: "skalp — Intent-Driven Hardware Description Language"
date: 2025-01-01
summary: "A modern HDL written in Rust that preserves design intent from algorithm to gates, with compile-time clock domain safety and progressive refinement. ~221K lines across 24 crates."
tags: ["hardware", "hdl", "rust", "fpga", "synthesis"]
ShowToc: true
---

**skalp** (from Sanskrit *संकल्पना — Sankalpana*, "conception with purpose") is a hardware description language I'm building in Rust. It sits between the tedium of RTL and the unpredictability of HLS, preserving design intent throughout the entire compilation pipeline.

[GitHub](https://github.com/girivs82/skalp) | ~221K lines of Rust | 24 workspace crates | 1,090+ commits

---

## Why I'm Building This

Hardware design in 2025 still runs on languages from the 1990s. SystemVerilog gives you control but drowns you in boilerplate — a FIFO takes 59 lines of careful bit-width arithmetic where one off-by-one means silent data corruption. HLS tools promise abstraction but produce unpredictable results you can't debug.

Both share deeper problems:

**Intent disappears.** You start with "I need a 100MHz pipelined multiplier" and end up hand-placing flip-flops. The *why* vanishes into the *how*. Six months later, no one — including you — remembers which constraints mattered.

**Clock domain crossings are discovered at 3 AM.** CDC bugs are the most dangerous class of hardware defect: they're intermittent, they pass simulation, and external verification tools (Spyglass, etc.) cost $50K+ per seat. Every project rediscovers the same bugs.

**There's no middle ground.** You either write cycle-accurate RTL from day one, or you use HLS and pray the tool makes reasonable architectural choices. There's no way to start with an algorithm and gradually add hardware constraints as you learn what matters.

**Verification is an afterthought.** You build first, test later. Assertions and formal properties are bolted on after the design exists, not woven into the design from the start.

skalp is my answer: a language where intent is a first-class type, clock domains are tracked by the compiler like Rust tracks memory lifetimes, and you can progressively refine from dataflow down to cycle-accurate RTL without starting over.

---

## Design Decisions

### Why Clock Domains as Lifetimes?

This is the decision I'm most proud of. In skalp, clock domains are part of the type system, modeled after Rust's lifetime annotations:

```
signal data: logic<'fast>[32]    // lives in the 'fast clock domain
signal sync: logic<'slow>[32]    // lives in the 'slow clock domain

sync = data;                      // COMPILE ERROR: clock domain mismatch
sync = synchronize(data);         // explicit CDC — compiler inserts synchronizer
```

The `'fast` and `'slow` lifetimes aren't decorative — they're tracked through expressions, assignments, and module boundaries. If you try to use a signal from one clock domain in another without explicit synchronization, the compiler rejects it. Not a lint warning. A hard error.

**Why not external tools?** Because compile-time catches 100% of crossings with zero cost, while external tools run post-synthesis and cost tens of thousands per seat. CDC bugs should be impossible to write, not expensive to find.

**Why not manual annotations?** Languages like Veryl support CDC annotations, but they're opt-in. You have to remember to annotate. skalp makes it structural — the type system won't let you forget.

When the compiler detects a crossing, the `#[cdc]` attribute specifies the synchronization strategy:

```
#[cdc(cdc_type = gray, sync_stages = 2, from = 'src, to = 'dst)]
signal write_ptr_gray: logic<'src>[8]
```

This generates proper Gray-code synchronizers in the SystemVerilog output, complete with synthesis attributes.

### Why Intent as a First-Class Feature?

Most compilers optimize using heuristics. skalp lets you declare what you actually want:

```
entity Accelerator {
    in data: stream<'clk>[32]
    out result: stream<'clk>[32]
} with intent {
    throughput: 100M_samples_per_sec,
    architecture: systolic_array,
    optimization: balanced(speed: 0.7, area: 0.3)
}
```

The intent system doesn't understand "throughput" as a keyword — it decomposes to primitive properties that guide optimization passes. This means the system is extensible without language changes. New intent types are library definitions, not grammar additions.

Intent is preserved through every IR layer. When the optimizer makes a tradeoff, it can check whether the result still satisfies the declared intent. When it doesn't, you get a clear error instead of silently degraded performance.

### Why Expression-Based Syntax?

skalp uses expression-based programming (like Rust) instead of statement-based (like Verilog):

```
result = match op {
    0b000 => a + b,
    0b001 => a - b,
    0b010 => a & b,
    _ => 0
};
```

The compiler checks exhaustiveness — if you forget a case, it tells you. Compare this with SystemVerilog's nested ternaries or case statements where a missing branch silently produces `x`.

This isn't just syntax sugar. Expression-based design composes naturally: you can inline results without intermediate variables, and pattern matching is the natural way to express state machines.

### Why Monomorphization Over Module Parameters?

skalp uses Rust-style monomorphization — each generic instantiation is specialized at compile time:

```
entity FIFO<const WIDTH: nat = 8, const DEPTH: nat = 16> {
    in wr_data: bit[WIDTH]
    out rd_data: bit[WIDTH]
    signal wr_ptr: nat[clog2(DEPTH)]     // compiler computes: clog2(16) = 4
    signal count: nat[clog2(DEPTH + 1)]   // compiler computes: clog2(17) = 5
}
```

`clog2(DEPTH)` is evaluated at compile time. No more `localparam ADDR_WIDTH = $clog2(DEPTH)` followed by `reg [ADDR_WIDTH-1:0] wr_ptr` where you have to remember the `-1`. The type-level computation handles it.

The tradeoff is compilation time for each instantiation, but the payoff is full type safety and const expression evaluation that SystemVerilog's parameter system can't express.

---

## Compiler Architecture

skalp uses a multi-layer IR approach inspired by LLVM, where each layer serves a distinct purpose:

```
Source (.sk / .skalp)
    ↓
Frontend: Lexer (logos) → Parser (rowan) → Type Checking
    ↓
HIR — Intent preserved, clock domains tracked, generics intact
    ↓
MIR — Cycle-accurate, architecture-independent, composites flattened
    ↓
LIR — Gate-level netlist with target primitives
    ↓
SIR — Simulation IR, GPU-optimized (separate path from synthesis)
    ↓
Backends: SystemVerilog · VHDL · Verilog · FPGA Bitstream
```

**Why four IRs?** Each has a clear purpose. HIR preserves everything from the source — your intent, your abstractions, your clock domain annotations. MIR is where optimization happens on cycle-accurate hardware with flattened types. LIR is the gate-level netlist that targets specific hardware. SIR is an entirely separate path optimized for GPU simulation (different data layout, different optimization goals).

### Frontend

The lexer uses the `logos` crate, which generates a state machine at compile time. One interesting challenge: disambiguating Rust-style lifetimes (`'clk`) from Verilog-style sized literals (`8'hFF`). The regex `'[a-zA-Z_&&[^bhd]][a-zA-Z0-9_]*` handles this — if the character after the apostrophe isn't `b`, `h`, or `d`, it's a lifetime.

The parser uses `rowan` for lossless syntax trees — every whitespace character and comment is preserved in the tree. This means the formatter (`skalp fmt`) can round-trip perfectly: parse → modify → emit produces identical output for unchanged regions. Error recovery is built in: invalid tokens are collected, not fatal, so the parser can report multiple errors per file.

The type checker uses a constraint-based approach (Hindley-Milner style). Rather than checking types immediately, it accumulates constraints — `TypeConstraint::Equal`, `TypeConstraint::WidthEqual`, `TypeConstraint::IsClock` — and solves them together. This enables cross-expression width inference: if you write `signal x = a + b`, the compiler infers the width of `x` from the widths of `a` and `b`.

### HIR → MIR: Where Abstractions Become Hardware

The most interesting transformation is **type flattening**. High-level types like structs and vectors can't exist in hardware, so MIR flattens them:

```
HIR:
  port vertex: struct { position: Vec3<f32>, color: bit[32] }

MIR (after flattening):
  port vertex_position_x: Float32
  port vertex_position_y: Float32
  port vertex_position_z: Float32
  port vertex_color: Bit(32)
```

But arrays of scalars are *preserved* — this is deliberate. An `array<bit[32], 1024>` stays as an array so the synthesis tool can choose the right implementation: BRAM for large arrays, distributed RAM for medium ones, registers for tiny ones. Flattening arrays would destroy this information.

CDC analysis runs at the MIR level, *before* optimization, so clock domain violations are caught before transformations could obscure them. SSA conversion eliminates combinational loops from mutable variables — `x = f(x)` becomes `x_1 = f(x_0)` — making the design safe for synthesis.

### Code Generation

SystemVerilog codegen maps MIR directly to synthesizable output. Float constants become IEEE 754 hex (`3.14159` → `32'h4048F5C2`). Memory arrays get synthesis attributes (`(* ram_style = "block" *)`) based on size heuristics. CDC crossings generate proper synchronizer chains with configurable stages.

One design choice worth noting: modules with unresolved generic types are *skipped* during codegen. Only concrete, monomorphized instantiations produce SystemVerilog. This prevents emitting invalid code for unspecialized templates.

---

## Simulation

The simulator uses a dependency-driven evaluation model. When a value changes, only the transitive closure of dependent signals (the "cone") needs re-evaluation:

```rust
// When a signal changes, mark its dependents as dirty
fn mark_dirty(&mut self, attr: AttributeId) {
    self.dirty_set.insert(attr.clone());
    for dep in self.reverse_deps.get(&attr) {
        self.mark_dirty(dep);  // recursive cone marking
    }
}

// Evaluate only dirty signals whose dependencies are clean
fn get_evaluation_batch(&mut self) -> Vec<AttributeId> {
    self.evaluation_order.iter()
        .filter(|attr| self.dirty_set.contains(attr))
        .filter(|attr| deps_are_clean(attr))
        .collect()
}
```

This cone-based approach is designed for GPU parallelization — the dependency graph can be precomputed on the device, and independent cones within a batch can execute as parallel GPU kernels. The architecture partitions circuits into simulation domains (SPICE, digital, behavioral) with explicit interfaces between them, so mixed-signal designs simulate correctly across domain boundaries.

---

## Equivalence Checking

One thing I wanted from the start: if the compiler transforms your design, you should be able to *prove* the transformation is correct, not just hope.

skalp includes a SAT-based equivalence checker in the formal verification crate. The approach:

1. Convert both designs (pre and post-transformation) to **And-Inverter Graphs** — a canonical bit-level representation where every operation is decomposed into 2-input ANDs and inversions
2. Build a **miter circuit** — XOR corresponding outputs, OR all the XORs together. If the miter can ever output 1, the designs differ
3. Encode to **CNF** using Tseitin transformation and hand it to a SAT solver
4. **UNSAT** = equivalent (no input exists that produces different outputs). **SAT** = counterexample found

```
Design A (pre-synthesis) ──→ AIG ──┐
                                    ├──→ Miter (XOR outputs) ──→ CNF ──→ SAT solver
Design B (post-synthesis) ──→ AIG ──┘
                                                                          │
                                                              UNSAT = equivalent
                                                              SAT = counterexample
```

This covers two use cases: **combinational equivalence** (same outputs for all inputs) and **sequential equivalence** using bounded model checking (same register behavior up to K cycles, with register matching by name, width verification, reset value checking, and next-state function comparison).

For large designs, FRAIG simplification (simulation + SAT sweeping) reduces the AIG before solving, and the SAT phase parallelizes across diff gates using rayon. The result either confirms equivalence or produces a concrete counterexample — actual input values that demonstrate the difference.

---

## Safety: Fault Injection and FMEDA

This is where skalp does something I haven't seen in other HDLs.

Traditional functional safety (ISO 26262) workflow: you design the hardware, hand it to a safety team, they manually build an FMEDA spreadsheet with assumed failure rates and estimated diagnostic coverage, and everyone hopes the numbers are right. DC values come from lookup tables, not measurement. It's slow, error-prone, and disconnected from the actual design.

skalp integrates fault injection into the compiler. You declare safety goals as intent, the compiler decomposes them to gate-level fault campaigns, injects faults into every primitive, measures what gets detected, and generates the FMEDA automatically with *measured* diagnostic coverage. Not estimated — measured.

### Fault Models

The fault injection system supports 14+ fault types organized by failure mechanism:

**Permanent faults** (manufacturing, wear-out): stuck-at-0, stuck-at-1, bridging, open

**Transient faults** (radiation, EMI): single-event upset, bit flip, multi-bit upset

**Timing faults** (margins, temperature): setup violation, hold violation, metastability

**Power faults** (analog effects on digital): voltage dropout (IR drop), ground bounce, crosstalk glitch

**Clock faults**: clock glitch (extra edge), clock stretch (PLL unlock)

Predefined fault sets map to ASIL levels — ASIL-A gets stuck-at only, ASIL-D gets the full set including power and clock faults.

### How DC Is Measured

You define failure effects as temporal conditions on observable signals:

```
// "valve output corrupted" if it equals 0xFFFF
effect valve_corrupted: valve_output == 0xFFFF (severity: S3)

// "watchdog timeout ignored" if timeout fires but CPU stays alive
effect timeout_ignored: @rose(timeout) && @stable(cpu_alive, 100)

// "TMR disagreement" across redundant sensors
effect sensor_disagree: @max_deviation(sensor_a, sensor_b, sensor_c) > 50
```

The condition language includes edge detection (`@rose`, `@fell`), stability checks (`@stable`), history (`@prev`, `@cycles_since`), arithmetic (`@abs_diff`, `@hamming_distance`), frequency analysis (`@pulse_count`, `@glitch_count`), and data integrity (`@crc32`, `@parity`).

During a fault campaign, every primitive in the design gets each fault type injected. For each injection, the simulator runs the test scenario and checks whether the fault caused a failure effect and whether a safety mechanism detected it. The result:

```
DC = faults_detected / faults_causing_effect
```

This is actual measurement, not a table lookup. If your safety mechanism detects 9,900 out of 10,000 faults that cause the "valve corrupted" effect, your DC for that effect is 99.0%. The system computes SPFM (Single Point Fault Metric), LFM (Latent Fault Metric), and PMHF (Probabilistic Metric for Hardware Failures) directly from simulation data.

### Common Cause Failure Analysis

The CCF analyzer identifies groups of components that share failure causes — same clock domain, same reset, same power rail, physical proximity, same cell type — and applies beta factors to split FIT rates into independent and correlated components:

```
SharedClock:     β = 0.07 (7% of failures are correlated)
SharedReset:     β = 0.05
SharedPower:     β = 0.07
PhysicalProximity: β = 0.01
SharedDesign:    β = 0.02 (systematic — same cell type)
SafetyMechanism: β = 1.0  (if SM fails, ALL protected logic is undetectable)
```

That last one matters most: when a safety mechanism itself fails, every component it protects becomes a single-point failure. The CCF analyzer identifies these SM-of-SM relationships automatically from the design hierarchy.

### Auto-Generated FMEDA

The output is a complete FMEDA with per-cell entries: base FIT rate (from tech library), failure distribution (safe/dangerous-detected/dangerous-undetected), measured DC (from fault injection), effective FIT breakdown (safe, SPF, residual, MPF), and the safety mechanism that provides detection. Gap analysis identifies exactly which primitives and fault types aren't meeting their ASIL targets, and how many additional detections are needed.

The GPU fault simulator targets 10–20M fault simulations per second on Apple Silicon, making exhaustive campaigns over tens of thousands of primitives practical in seconds rather than hours.

### Why This Matters

This turns FMEDA from a late-stage manual audit into a design-time feedback loop. You change a safety mechanism, re-run the fault campaign, and see immediately whether DC improved or regressed. The safety case is built from evidence, not assumptions.

---

## What Makes This Different

The philosophical difference from other modern HDLs (like Veryl or Chisel):

**Veryl** is "SystemVerilog, but better" — evolutionary. It cleans up the syntax and adds conveniences, but transpiles to SystemVerilog and relies on its semantics. skalp is a new compilation target that *generates* SystemVerilog as one of several backends.

**Chisel** embeds hardware description in Scala. This gives you Scala's type system but also Scala's complexity and JVM dependency. skalp is a standalone language with its own type system designed specifically for hardware concerns (clock domains, bit widths, synthesis constraints).

**skalp's bet**: clock domain safety and intent preservation are worth a new language. Not a better syntax for an old language — a fundamentally different compilation model where the things that cause the worst bugs (CDC violations, lost intent, width mismatches) are structurally impossible.

| | skalp | SystemVerilog | VHDL | Chisel | Veryl |
|---|---|---|---|---|---|
| CDC safety | Compile-time (type system) | None | None | None | Manual annotations |
| Intent preservation | First-class, through all IRs | None | None | None | None |
| Type safety | Strong, with inference | Weak | Strong | Strong (Scala) | Moderate |
| Width arithmetic | Const expressions (`clog2`) | Manual, error-prone | Manual | Scala expressions | Basic |
| Equivalence checking | Built-in (AIG + SAT) | External tools | External tools | None | None |
| Fault injection / FMEDA | Integrated, measured DC | None | None | None | None |
| Syntax | Rust-inspired, expression-based | C-like, statement-based | Verbose | Scala DSL | Rust-inspired |
| Output | SV, VHDL, Verilog, bitstream | Native | Native | Verilog | SystemVerilog |

---

## Project Structure

```
crates/
  skalp-frontend/    Lexer, parser, type checker, HIR (logos + rowan)
  skalp-mir/         Mid-level IR, optimization passes, CDC analysis
  skalp-lir/         Low-level IR, gate-level netlist
  skalp-codegen/     SystemVerilog / VHDL / Verilog generation
  skalp-sim/         Simulation engine (cone-based, GPU-ready)
  skalp-sir/         Simulation IR with GPU memory layout
  skalp-place-route/ Native FPGA place & route (iCE40)
  skalp-backends/    FPGA and ASIC synthesis backends
  skalp-safety/      ISO 26262 FMEDA and ASIL analysis
  skalp-formal/      Formal verification and model checking
  skalp-stdlib/      Standard library (bitops, FP, vectors, math)
  skalp-lint/        Hardware-aware linter (10 categories)
  skalp-lsp/         Language server for VS Code / Neovim
  skalp-ml/          ML-guided synthesis optimization (ONNX)
  skalp-parallel/    Parallel compilation engine
  skalp-incremental/ Incremental build system
  skalp-package/     Package manager
  ...                (24 crates total)

examples/
  counter.sk         Simple counter
  fifo.sk            FIFO buffer
  alu.sk             Arithmetic logic unit
  real_world/        UART, SPI, I2C, AXI4-Lite, memory arbiter
  ncl/               Null Convention Logic (async circuits)
  graphics_pipeline/ GPU-like pipeline components
```

---

## Current Status

The compiler pipeline from source through HIR, MIR, and SystemVerilog codegen is functional. The frontend parses the full language grammar, the type checker catches CDC violations and width mismatches, and the codegen produces synthesizable SystemVerilog with proper synchronizers and memory inference.

The LSP server, formatter, linter, and package manager are implemented. Simulation is architecturally complete with the cone-based evaluation model but GPU acceleration (Metal on macOS) is still being integrated into the runtime.

Active work is focused on expanding the standard library, completing the GPU simulation backend, and building out the documentation.
