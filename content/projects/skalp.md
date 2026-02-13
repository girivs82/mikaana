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

In practice, the equivalence checker has been one of the most valuable debugging tools in the project. Running EC between the simulator and synthesis backends caught a significant number of bugs in both — cases where the simulator computed the wrong value for an edge case, or where a synthesis optimization silently changed behavior. Having a formal proof that two representations agree (or a concrete counterexample when they don't) turns "it seems to work" into "it provably works."

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

## Standard Library: Types as Library, Not Language

Most HDLs bake their type systems into the language. Want a new floating-point format? Wait for the next language revision. skalp takes a different approach: the type system is expressive enough that complex types like floating-point are *library definitions*, not language primitives.

### Floating-Point Is Not Built In

`fp32` in skalp is not a keyword — it's a set of stdlib functions that operate on `bit[32]` according to IEEE 754 layout:

```
// fp32 is just bit manipulation on a 32-bit vector
pub fn fp32_sign(x: bit[32]) -> bit[1] { x[31:31] }
pub fn fp32_exp(x: bit[32]) -> bit[8] { x[30:23] }
pub fn fp32_mantissa(x: bit[32]) -> bit[23] { x[22:0] }

pub fn fp32_pack(sign: bit[1], exp: bit[8], mantissa: bit[23]) -> bit[32] {
    (sign as bit[32] << 31) | (exp as bit[32] << 23) | mantissa as bit[32]
}
```

Multiplication, addition, comparison, classification — all built on top of these primitives as synthesizable hardware operations. The same pattern defines `fp16` (1/5/10), `fp64` (1/11/52), and will define `bfloat16` and `tf32` for ML workloads.

**Why this matters:** if you need a custom 24-bit float format for your specific application, you define it in your own library using the same mechanisms the stdlib uses. You're not waiting for a language update — you're writing library code. And because the compiler sees the bit-level operations, it can optimize them the same way it optimizes any other hardware.

### The Trait System

Traits define what a type can do in hardware:

```
trait FloatingPoint {
    const WIDTH: nat
    const EXP_WIDTH: nat
    const MANT_WIDTH: nat

    fn add(self, other: Self) -> Self
    fn mul(self, other: Self) -> Self
    fn is_nan(self) -> bit
    fn zero() -> Self
}
```

Generic entities use trait bounds to work with any conforming type:

```
entity Vec2Add<T> where T: Synthesizable {
    in a: vec2<T>
    in b: vec2<T>
    out result: vec2<T>
}
```

This is how the stdlib defines vector operations that work across `fp32`, `fp16`, fixed-point, or any user-defined numeric type. One implementation, any element type, fully specialized at compile time through monomorphization.

The stdlib has no special privileges. You can implement the `FloatingPoint` trait for your own type — a custom 24-bit float for your ML accelerator, a posit format, a logarithmic number system — and every generic entity in the stdlib that uses `where T: FloatingPoint` works with it automatically. You can also replace stdlib implementations entirely: if the default `fp32_mul` doesn't meet your area or timing goals, write your own and use it instead. The stdlib is a starting point, not a ceiling.

### What's in the Standard Library

**Floating-point** (fp16, fp32, fp64): full IEEE 754 arithmetic, comparison, classification. Transcendental functions — sin, cos, tan, atan2, ln, exp, pow, sqrt — implemented as Newton-Raphson iterations and Taylor series approximations, all synthesizable to RTL. Fast inverse sqrt uses the Quake III algorithm adapted for hardware.

**Fixed-point** (Q15.16, Q31.32): add, subtract, multiply with saturation arithmetic. Conversions to and from floating-point. Overflow detection.

**Vectors** (vec2, vec3, vec4): component-wise arithmetic, dot product, cross product, normalize (accurate and fast variants), reflect, project, reject, lerp, distance. The Phong and Blinn-Phong shading examples in the repo are built entirely from these stdlib operations.

**Bit manipulation**: clz, ctz, popcount, bitreverse, ffs, fls, parity, sign extension, power-of-2 checks, Gray code encoding/decoding, byte swapping, alignment utilities, bitfield extract/insert.

**Math**: min, max, abs, clamp, lerp, smoothstep, FMA/FMS, floor, ceil, round, fract, modulo.

**Reusable components**: parameterized adders, counters, FIFOs, shifters, multiplexers — each generic over width and depth.

**Interface protocols**: AXI4, AXI4-Lite, Avalon MM, Wishbone bus definitions.

### The Design Principle

The stdlib is built on composition. `clamp` is composed of `max` and `min`. `normalize` uses `dot`, `rsqrt`, and scalar multiply. Nothing is magic — you can read the implementation of any stdlib operation and see the hardware it generates.

The boundary between language and library is deliberate: the *language* provides the type system, generic instantiation, trait bounds, and synthesis semantics. The *library* provides the types themselves, their operations, and hardware-specific implementations. This keeps the language small and the ecosystem extensible.

---

## Synthesis: From Words to Gates

The synthesis backend lives in `skalp-lir` and `skalp-backends`. It takes the MIR and lowers it through a word-level intermediate (LIR) to a gate-level netlist, then optimizes that netlist using an ABC-inspired AIG optimization engine.

### Why a Word-Level LIR?

Most synthesis flows eagerly decompose everything to individual bits before optimization. skalp deliberately preserves multi-bit operations in the LIR:

```
LIR: Add { width: 8, has_carry: true }
     Mux2 { width: 32, sel_pos: 0 }
     Reg { width: 16, reset_value: 0 }
```

Why? Technology libraries may have compound cells — `ADDER8`, `DPMUX4`, `AOI22` — that directly implement multi-bit operations. If you decompose to bits before mapping, you lose the chance to use them. The mapper decomposes as needed during technology mapping, falling back to per-bit logic when compound cells aren't available.

### Technology Mapping with Truth Tables

The mapper assigns LIR operations to library cells by matching truth tables. Each cell function is encoded as a truth table with input permutations:

```
And2  → 0x8 (1000b)     Nand2 → 0x7 (0111b)
Or2   → 0xE (1110b)     Xor2  → 0x6 (0110b)
Aoi21 → 0x15            Mux2  → 0xCA (with 6 permutations)
```

When a direct match isn't available, the mapper tries inversion absorption — implementing the inverted function with fewer gates (NAND instead of AND + inverter). Multi-input gates are handled by enumerating input permutations and matching against the library's available cells.

Multi-bit signals expand to per-bit nets (e.g., `result[7]`, `result[0]`), but the expansion happens at the mapping boundary, not the IR level. This keeps the optimization pipeline working at the word level as long as possible.

### AIG Optimization Engine

After technology mapping, the gate netlist is converted to an **And-Inverter Graph** — where every operation is decomposed to 2-input ANDs with inverted literals — and run through ABC-inspired optimization passes:

**FRAIG (Functionally Reduced AIG):** SAT-based equivalence detection. Simulates 64-bit random patterns to identify candidate equivalent nodes, then proves or disproves equivalence via SAT solving (checking if `node₁ XOR node₂` is UNSAT). Counterexamples from SAT refine the equivalence classes. Configurable conflict limits (1,000 per SAT call, 10,000 total) prevent runaway solving on hard instances.

**Register retiming:** Leiserson-Saxe algorithm for moving registers across combinational logic to balance path delays. Configurable target period (default 10ns/100MHz, with a `high_frequency()` preset targeting 2ns/500MHz). Supports both forward and backward retiming.

**Balance:** Reduces AIG depth by restructuring the AND tree. Shorter depth means fewer logic levels and higher clock frequency.

**Rewrite and Refactor:** Pattern-based and structural rewriting passes that replace subgraphs with functionally equivalent but smaller or faster alternatives.

**Constant propagation and DCE:** Standard compiler passes adapted for hardware — propagate known values and eliminate dead logic.

These compose into synthesis presets:

| Preset | Strategy |
|--------|----------|
| Quick | Minimal passes for fast turnaround |
| Balanced | Default — good quality-of-results vs. runtime |
| Full | Maximum effort, all passes |
| Timing | Prioritize meeting clock constraints |
| Area | Minimize gate count |
| Resyn2 | ABC's proven sequence: balance → rewrite → refactor → balance → rewrite → rewrite‑z → balance → refactor‑z → rewrite‑z → balance |
| Compress2 | ABC's area-focused script with resubstitution |
| Auto | Run multiple presets in parallel, pick best result |

### Cell Sizing

After mapping, cells are upsized based on fanout to ensure adequate drive strength:

```
≤2 fanout → X1 (base drive)
≤4 fanout → X2
≤8 fanout → X4
≤16 fanout → X8
```

Timing-driven sizing upsizes cells on critical paths when slack falls below a target threshold.

### Power Domain Barriers

In the AIG, power domain crossings are represented as **barrier nodes** — level shifters, isolation cells, retention flip-flops, power switches — that the optimizer is forbidden from optimizing through. This prevents the synthesis engine from accidentally simplifying logic across power domain boundaries, which would break isolation.

The barrier types include: level shifters (low→high and high→low), always-on buffers, isolation cells (AND/OR/latch variants), retention DFFs, power switches (PMOS header, NMOS footer), and I/O pads (input, output, bidirectional, clock, analog). Each carries enable signals and reset connections appropriate to its function.

### NCL (Null Convention Logic) Support

The mapper has first-class support for asynchronous circuits using Null Convention Logic. When it detects dual-rail signals (names ending in `_t` for true rail, `_f` for false rail), it maps AND operations to C-elements (threshold gates, TH22) instead of regular AND gates. If the target library has TH22 cells, they're used directly; otherwise, the mapper synthesizes a C-element from standard logic: `Q = (a & b) | (Q & (a | b))`.

### Target Platforms

The backend supports multiple targets through a unified configuration interface:

**FPGA:** Lattice iCE40 (4-input LUTs, carry chains), Xilinx 7-Series (6-input LUTs, DSP slices, hardened multipliers), Intel Cyclone V

**ASIC:** FreePDK45 (open-source 45nm), SkyWater 130nm (open-source 130nm), and generic standard cell libraries via Liberty (.lib) and LEF files

Each target defines its primitive library, and the tech mapper selects cells accordingly. Library cells carry timing arcs across seven process corners (TT, SS, FF, SF, FS, SSLV, FFHV) for multi-corner timing analysis, voltage sensitivity rankings for brownout simulation, and FIT rates for safety analysis — all flowing through to the FMEDA.

---

## Place and Route: From Netlist to Bitstream

skalp includes a native place-and-route engine (`skalp-place-route`) targeting iCE40 FPGAs. Rather than depending on vendor tools, the P&R generates IceStorm-compatible bitstreams directly — from gate-level netlist to programming file in a single toolchain.

### The Pipeline

```
Gate Netlist
    ↓
Packing — combine LUT+DFF cells into logic cells
    ↓
Placement — assign cells to physical locations on the FPGA
    ↓
Routing — connect cells through the routing fabric
    ↓
Timing Analysis — compute critical paths, check constraints
    ↓
Bitstream Generation — produce IceStorm ASCII format
```

### Placement

The placer implements seven algorithms, selectable per design:

**Analytical placement** solves a quadratic wirelength minimization problem using conjugate gradient. It builds a Laplacian connectivity matrix from the netlist — each net contributes edge weights inversely proportional to its fanout (clique model) — then solves `Lx = b` for X and Y coordinates simultaneously. I/O cells are anchored to chip boundaries with 100x weight to keep them at the edges. The result is continuous coordinates that get snapped to valid BEL sites during legalization.

**Simulated annealing** starts from an initial placement and iteratively proposes swaps (exchange two cells) or relocations (move a cell to a new site). Each move is evaluated using half-perimeter wirelength (HPWL), accepted or rejected via Boltzmann probability `P = exp(-ΔCost / T)`, and the temperature cools geometrically. The implementation supports **parallel move evaluation** using Rayon — batches of independent moves are evaluated concurrently, significantly reducing runtime for large designs.

**Hybrid approaches** combine both: analytical placement produces a good starting point, legalization snaps it to valid sites, then simulated annealing refines locally. Timing-driven variants weight moves by net criticality, biasing 30% of SA moves toward cells on critical paths.

**Legalization** uses an expanding ring search: starting from the analytical solution's coordinates, it searches outward ring-by-ring for the nearest compatible, unoccupied BEL. The BEL compatibility matrix handles the fact that in iCE40, all flip-flop variants (DFF, DFFE, DFFSR, DFFSR+E) map to the same hardware with different configuration bits.

### Routing

Routing uses a three-phase approach:

**Phase 1: Global nets.** Clocks and resets are routed through the 8 dedicated GBUF (Global Buffer) networks first. These have near-zero skew and minimal delay (~50ps) but are a limited resource. Nets that can't fit in global networks fall back to regular routing.

**Phase 2: Carry chains.** Dedicated carry chain wires connect adjacent logic cells vertically. These are deterministic (fixed connectivity) and handled before regular routing to avoid congestion on the dedicated resources.

**Phase 3: Regular nets via PathFinder.** The core routing algorithm is **PathFinder with A\* search** — a negotiated congestion approach where nets compete for shared routing resources across multiple iterations:

1. Route all nets using A* shortest-path with Manhattan distance heuristic
2. Identify congested wires (usage > capacity)
3. Rip up nets that use congested wires
4. Increase history costs on congested wires
5. Reroute with updated costs
6. Repeat until no congestion remains

The cost function balances three components:

```
cost = base_pip_cost × congestion_multiplier + delay_contribution
```

Where congestion is `present_factor × (1 + overuse)` for overused wires (present factor = 1.5), plus accumulated history cost from previous iterations (history factor = 1.0). The history cost prevents the router from oscillating between the same bad solutions — once a wire is congested, it stays expensive even after rip-up.

A* explores the routing graph through **PIPs** (Programmable Interconnect Points) — configurable switches that connect one wire to another. Each PIP has a base cost and delay. Timing-driven routing adds delay contribution to the cost function, weighting it by net criticality.

### iCE40 Architecture Model

The device database models the complete iCE40 architecture:

**Variants:** HX1K (13×17 grid, 1280 LUTs), HX4K (17×17, 3520 LUTs), HX8K (33×33, 7680 LUTs), plus LP (low-power) equivalents and UP5K (25×21, 5280 LUTs with DSP blocks)

**Tile types:** Logic (8 LUTs + 8 FFs + carry chain), I/O (top/bottom/left/right), RAM, Global Buffer, PLL, DSP

**Wire types:** Local (within tile), Span-4 (4-tile horizontal/vertical), Span-12 (long lines), Neighbour (adjacent tiles), Carry Chain (dedicated vertical), Global (8 clock networks)

The device loads from real IceStorm chipdb files when available, mapping BEL pins to wire IDs and constructing the full PIP connectivity graph. A synthetic fallback generates the architecture model from variant parameters when chipdb files aren't present.

### Bitstream Generation

The output is IceStorm ASCII format — a text representation of the FPGA configuration that IceStorm tools (`icepack`) convert to binary bitstream. Each logic tile is a 16×54 bit matrix encoding LUT truth tables (16 bits per logic cell), DFF configuration (negative clock, carry enable, DFF enable, set/reset mode), and routing switch settings. I/O tiles encode pin type (input mode, output select, tristate control, pull-up enable). RAM tiles encode memory initialization and port configuration.

The generator also produces a utilization report with resource usage, timing summary, and critical path information.

### Timing Analysis

Static timing analysis uses variant-specific delay models:

| Component | HX | LP | UP |
|-----------|-----|-----|-----|
| LUT4 | 0.54ns | 0.65ns | 0.70ns |
| DFF clk-to-Q | 0.85ns | 0.85ns | 0.85ns |
| DFF setup | 0.18ns | 0.18ns | 0.18ns |
| Carry (per bit) | 0.09ns | 0.09ns | 0.09ns |
| Local wire | 0.05ns | 0.05ns | 0.05ns |
| Span-4 | 0.20ns | 0.20ns | 0.20ns |
| Span-12 | 0.40ns | 0.40ns | 0.40ns |
| RAM read | 3.50ns | 3.50ns | 3.50ns |

The analyzer finds clock domains, builds a timing graph from placement and routing data, and reports worst negative slack, failing paths, and achievable frequency.

---

## What Makes This Different

Most modern HDL efforts improve the *language* while leaving the *toolchain* unchanged. skalp is a complete toolchain — language, compiler, synthesis, place & route, simulation, formal verification, and safety analysis — where each piece is designed to work with the others.

**Veryl** is "SystemVerilog, but better" — evolutionary. It cleans up the syntax and adds conveniences, but transpiles to SystemVerilog and relies on external tools for everything after code generation: synthesis, simulation, formal, safety. skalp owns the full pipeline from source to bitstream.

**Chisel** embeds hardware description in Scala. This gives you Scala's type system but also Scala's complexity and JVM dependency. It generates Verilog and hands off to vendor tools. There's no integrated equivalence checking, no fault injection, no safety analysis.

**SystemVerilog and VHDL** are the industry workhorses, but the toolchain is a patchwork: one vendor's synthesis, another's simulation, a third-party formal tool, manual FMEDA spreadsheets, separate CDC analysis at $50K/seat. Each tool has its own model of the design. Nothing is proven consistent across them.

**skalp's difference** is that everything lives in one compilation model:

- The **type system** catches CDC violations at compile time — not as a post-synthesis lint, but as a hard error before any hardware is generated
- **Intent** is preserved through every IR, so optimization passes can check whether they're violating your constraints, not just minimizing area blindly
- **Equivalence checking** runs between the simulator and synthesis backends, proving transformations correct — and in practice, this has been one of the most effective tools for finding bugs in both
- **Fault injection** produces measured diagnostic coverage from actual simulation, not estimated DC from lookup tables — turning FMEDA from a manual audit into a design-time feedback loop
- **Synthesis** maps through a word-level LIR to preserve compound cell opportunities, optimizes via AIG passes (FRAIG, retiming, rewrite), and supports both FPGA and ASIC targets with multi-corner timing
- **Place and route** takes the gate netlist all the way to an iCE40 bitstream — analytical placement, PathFinder routing, timing analysis, IceStorm output — without leaving the toolchain
- The **standard library** defines types (including all floating-point formats) as library code, not language primitives, using a trait system that makes every generic entity work with user-defined types

skalp is also the only tooling ecosystem with first-class support for **Null Convention Logic** — asynchronous, clockless circuits using dual-rail encoding and threshold gates. No other HDL or synthesis tool provides integrated NCL support: from language-level dual-rail signal declaration through synthesis (C-element mapping, TH22 threshold gates) to place and route. If you're designing delay-insensitive or self-timed circuits, there is currently no other option with end-to-end tooling.

The bet is that a unified toolchain catches entire classes of bugs that fall through the cracks of a fragmented one. When the same compiler that checks your clock domains also runs your fault campaigns and proves your synthesis correct, the pieces reinforce each other instead of operating in isolation.

| | skalp | SystemVerilog | VHDL | Chisel | Veryl |
|---|---|---|---|---|---|
| CDC safety | Compile-time (type system) | None | None | None | Manual annotations |
| Intent preservation | First-class, through all IRs | None | None | None | None |
| Type safety | Strong, with inference | Weak | Strong | Strong (Scala) | Moderate |
| Width arithmetic | Const expressions (`clog2`) | Manual, error-prone | Manual | Scala expressions | Basic |
| Equivalence checking | Built-in (AIG + SAT) | External tools | External tools | None | None |
| Fault injection / FMEDA | Integrated, measured DC | None | None | None | None |
| Syntax | Rust-inspired, expression-based | C-like, statement-based | Verbose | Scala DSL | Rust-inspired |
| Synthesis | Built-in (AIG, tech mapping, cell sizing) | External tools | External tools | External tools | External tools |
| Async / NCL | First-class (dual-rail, C-elements, TH gates) | None | None | None | None |
| Place & Route | Native (iCE40, bitstream gen) | External tools | External tools | External tools | None |
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

The synthesis backend is implemented with AIG optimization, technology mapping, and cell sizing across multiple target libraries. The native place-and-route engine targets iCE40 FPGAs with analytical and simulated annealing placement, PathFinder routing, and IceStorm bitstream generation.

The LSP server, formatter, linter, package manager, and GPU-accelerated simulation backend (Metal on macOS) are implemented. The standard library covers floating-point, fixed-point, vectors, bit manipulation, math, and reusable components with full trait-based extensibility.
