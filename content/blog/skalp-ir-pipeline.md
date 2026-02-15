---
title: "Four IRs Deep: How skalp Compiles Hardware"
date: 2026-02-15
summary: "skalp compiles hardware descriptions through four intermediate representations — HIR preserves intent, MIR models cycle-accurate RTL, LIR maps to technology primitives, and SIR optimizes for GPU simulation. How each IR serves a different purpose, what gets lowered at each stage, and why the synthesis and simulation paths diverge at LIR."
tags: ["skalp", "compiler", "ir", "architecture"]
ShowToc: true
---

Software compilers have one IR. LLVM's entire ecosystem — Clang, Rust, Swift, Julia — lowers everything to LLVM IR, optimizes there, and emits machine code. One representation, one optimization pipeline, many targets. It's elegant and it works because software has one execution model: a sequential thread (or a few) executing instructions on a register file with a memory hierarchy.

Hardware doesn't have one execution model. A hardware description needs to express intent ("this is a 3-stage pipeline with hazard detection"), cycle-accurate behavior ("on the rising edge of clk, if enable is high, register the output"), technology mapping ("use a carry-lookahead adder from the TSMC 7nm library"), and parallel execution ("evaluate these 5000 combinational cones simultaneously on a GPU"). No single representation can serve all four concerns without being either too abstract for some or too concrete for others.

skalp has four intermediate representations:

- **HIR** (High-level IR): The designer's intent. Generics, pipeline annotations, safety attributes, clock domain specifications. This is where `#[pipeline(stages=3)]` is a single annotation, not three pipeline registers.

- **MIR** (Mid-level IR): Cycle-accurate RTL. Processes with sensitivity lists, flip-flops with clock edges, module instances with port connections. This is where skalp generates SystemVerilog — every signal, every register, every `always_ff` block.

- **LIR** (Low-level IR): Technology primitives. Word-level operations that map to standard cell library entries. Adders, multiplexers, flip-flops, NCL threshold gates. The representation that technology mappers consume.

- **SIR** (Simulation IR): GPU-optimized execution. Flat, topologically sorted, with separate combinational and sequential node lists. The representation that [SharedCodegen](/blog/gpu-accelerated-simulation/#the-sharedcodegen-architecture) turns into Metal shaders and compiled C++.

This post covers what each IR looks like, what gets lowered at each transition, and why the pipeline splits into two paths after MIR.

---

## The Pipeline at a Glance

The four IRs form two paths that share a common prefix and diverge after MIR:

```
                ┌───────────────────────────────────────────────────────┐
                │                  skalp Source (.sk)                    │
                └───────────────────────┬───────────────────────────────┘
                                        │ parse + resolve
                                        ▼
                              ┌──────────────────┐
                              │       HIR        │
                              │  (intent, types, │
                              │   safety attrs)  │
                              └────────┬─────────┘
                                       │ lower intent → RTL
                                       ▼
                              ┌──────────────────┐
                              │       MIR        │
                              │ (cycle-accurate  │
                              │  RTL, hierarchy) │
                              └──┬───────────┬───┘
                                 │           │
              ┌──────────────────┘           └──────────────────┐
              │ synthesis path                simulation path   │
              ▼                                                 ▼
    ┌──────────────────┐                              ┌──────────────────┐
    │       LIR        │                              │       SIR        │
    │  (word-level     │                              │  (flat, sorted,  │
    │   primitives)    │                              │   GPU-ready)     │
    └──┬───────────┬───┘                              └──┬───────────┬───┘
       │           │                                     │           │
       ▼           ▼                                     ▼           ▼
  ┌─────────┐ ┌─────────┐                         ┌─────────┐ ┌─────────┐
  │SystemV. │ │Bitstream│                         │ Metal   │ │Compiled │
  │ (.sv)   │ │ (.bit)  │                         │ shader  │ │ C++     │
  └─────────┘ └─────────┘                         └─────────┘ └─────────┘
```

Three paths through the pipeline:

**Synthesis** — HIR → MIR → LIR → SystemVerilog/Bitstream. The traditional hardware compilation path. HIR captures intent, MIR makes it cycle-accurate, LIR maps to technology primitives, and the backend emits either SystemVerilog for ASIC flows or bitstreams for FPGA targets.

**Behavioral simulation** — HIR → MIR → SIR → Metal/C++. For simulating the design at RTL level. MIR is converted directly to SIR, preserving multi-bit semantics (a 32-bit add stays a single node, not 32 full adders). SharedCodegen produces Metal compute shaders for GPU execution and compiled C++ for CPU execution.

**Gate-level fault simulation** — HIR → MIR → LIR → SIR → Metal/C++. For ISO 26262 fault campaigns. The design goes through LIR first (decomposing to gate-level primitives), then LIR→SIR produces a flat primitive list that the GPU fault simulator dispatches one-thread-per-fault. Each primitive has a type encoding, input/output net IDs, and the fault injector modifies outputs per-cycle.

The divergence at MIR is the central architectural decision. Behavioral simulation doesn't need gate-level decomposition — it would make GPU simulation dramatically slower by turning one 32-bit add instruction into hundreds of gate evaluations. Fault simulation does need gate-level decomposition — the whole point is injecting faults at individual gates. Same source, same HIR, same MIR, but different downstream representations optimized for different execution models.

---

## HIR: Intent and Dataflow

HIR is the first representation after parsing. It preserves everything the designer wrote — generics, type aliases, pipeline annotations, safety attributes, clock domain declarations — without committing to implementation choices.

The core type is `HirEntity`, which represents a hardware module at the intent level:

```rust
pub struct HirEntity {
    pub id: EntityId,
    pub name: String,
    pub is_async: bool,
    pub visibility: HirVisibility,
    pub ports: Vec<HirPort>,
    pub generics: Vec<HirGeneric>,
    pub clock_domains: Vec<HirClockDomain>,
    pub assignments: Vec<HirAssignment>,
    pub signals: Vec<HirSignal>,
    pub span: Option<SourceSpan>,
    pub pipeline_config: Option<PipelineConfig>,
    pub vendor_ip_config: Option<VendorIpConfig>,
    pub power_domains: Vec<HirPowerDomain>,
    pub power_domain_config: Option<PowerDomainConfig>,
}
```

What's in HIR that won't survive lowering:

**Generics.** An `HirGeneric` can be a type parameter (`T`), a width parameter (`N`), a clock domain parameter, or a power domain parameter. HIR preserves these as parameters — the entity is still polymorphic. After lowering to MIR, every generic is monomorphized: `Fifo<32, 8>` becomes a concrete module with 32-bit data and 8-entry depth, and the generic parameters are gone.

```rust
pub struct HirGeneric {
    pub name: String,
    pub param_type: HirGenericType,
    pub default_value: Option<HirExpression>,
}

pub enum HirGenericType {
    Type,
    TypeWithBounds(Vec<String>),
    Const(HirType),
    Width,
    ClockDomain,
    PowerDomain,
}
```

**Pipeline configuration.** A `#[pipeline(stages=3)]` annotation in the source becomes a `PipelineConfig` in HIR. This is a single declaration of intent — "this entity should be pipelined with 3 stages." The MIR lowering turns this into explicit pipeline registers, stage enables, and hazard detection logic. By MIR, the pipeline is no longer an annotation but a concrete implementation with specific flip-flops and mux chains.

**Safety attributes.** ISO 26262 annotations live in HIR as `DetectionConfig` on ports and signals, `PowerDomainConfig` for power island specifications, and safety-specific metadata. A port marked with `#[detection(mode = "continuous")]` carries that annotation through HIR. In MIR, it becomes a `SafetyContext` attached to the module. In LIR, it becomes `LirSafetyInfo` on individual primitives. In SIR, detection signals are tracked separately for the fault simulator.

**Ports carry type information that MIR will flatten:**

```rust
pub struct HirPort {
    pub id: PortId,
    pub name: String,
    pub direction: HirPortDirection,
    pub port_type: HirType,
    pub physical_constraints: Option<PhysicalConstraints>,
    pub detection_config: Option<DetectionConfig>,
    pub power_domain_config: Option<PowerDomainConfig>,
}
```

An `HirType` can be a struct, enum, union, array, or parameterized type. A port of type `struct { valid: bit, data: bit<32>, tag: bit<4> }` is one port in HIR but becomes three flat signals in MIR (`port_valid`, `port_data`, `port_tag`). The struct type is a convenience for the designer; the hardware doesn't have structs.

**Signals can carry memory configuration, trace configuration, and detection configuration:**

```rust
pub struct HirSignal {
    pub id: SignalId,
    pub name: String,
    pub signal_type: HirType,
    pub initial_value: Option<HirExpression>,
    pub clock_domain: Option<ClockDomainId>,
    pub span: Option<SourceSpan>,
    pub memory_config: Option<MemoryConfig>,
    pub trace_config: Option<TraceConfig>,
    pub detection_config: Option<DetectionConfig>,
    pub power_domain: Option<String>,
}
```

A signal with `memory_config` is a memory array — the designer declares `let mem: Memory<32, 256>` and HIR records the width and depth. MIR will represent this as an array-typed signal. LIR will decompose it into `MemCell` primitives. SIR will represent it as a `Memory` node with `ArrayRead`/`ArrayWrite` operations.

**The expression and statement system is rich.** HIR has `match` expressions (not just `case` — pattern matching with enum variants, wildcards, and guards), `for` loops with generate semantics (`GenerateFor`, `GenerateIf`, `GenerateMatch`), function calls, field access, and type-qualified enum variants. Much of this complexity is lowered away in MIR:

- `match` expressions become `case` statements with explicit value comparisons
- `for` loops with `generate` semantics are unrolled
- Function calls are inlined (up to `MAX_INLINE_CALL_COUNT = 5` calls deep)
- Enum variants become integer constants
- Struct field access becomes bit-range selection on flat signals

HIR is the last representation where the designer's intent is fully preserved. After lowering to MIR, you can still reconstruct what the hardware does, but not necessarily why the designer structured it that way.

---

## MIR: Cycle-Accurate RTL

MIR is skalp's cycle-accurate representation. It's the level at which SystemVerilog is generated, and it corresponds closely to what RTL engineers think of as "the design." Every signal is declared, every flip-flop has an explicit clock and reset, every combinational dependency is visible in a process.

The top-level container is `Mir`, which holds a list of modules and safety definitions:

```rust
pub struct Mir {
    pub name: String,
    pub modules: Vec<Module>,
    pub safety_definitions: ModuleSafetyDefinitions,
}
```

Each `Module` is a hardware module with ports, signals, processes, instances, and metadata:

```rust
pub struct Module {
    pub id: ModuleId,
    pub name: String,
    pub parameters: Vec<GenericParameter>,
    pub ports: Vec<Port>,
    pub signals: Vec<Signal>,
    pub variables: Vec<Variable>,
    pub processes: Vec<Process>,
    pub assignments: Vec<ContinuousAssign>,
    pub instances: Vec<ModuleInstance>,
    pub clock_domains: Vec<ClockDomain>,
    pub generate_blocks: Vec<GenerateBlock>,
    pub assertions: Vec<Assertion>,
    pub span: Option<SourceSpan>,
    pub pipeline_config: Option<PipelineConfig>,
    pub safety_context: Option<SafetyContext>,
    pub is_async: bool,
    // ...
}
```

**Ports and signals are concrete.** No generics, no structs, no parameterized widths. Every port has a fixed `DataType` with a known bit width:

```rust
pub struct Port {
    pub id: PortId,
    pub name: String,
    pub direction: PortDirection,
    pub port_type: DataType,
    pub physical_constraints: Option<PhysicalConstraints>,
    pub span: Option<SourceSpan>,
    pub detection_config: Option<DetectionConfig>,
}
```

The `DataType` enum is rich — `Bit(usize)`, `Logic(usize)`, `Int(usize)`, `Bool`, `Float16`/`Float32`/`Float64`, `Vec2`/`Vec3`/`Vec4`, `Array`, `Struct`, `Enum`, `Ncl(usize)` for async dual-rail. But by MIR, every parameterized width has been resolved. A `Bit(BitParam { param: "N" })` in HIR becomes `Bit(32)` in MIR after monomorphization.

**Processes are where behavior lives.** A `Process` is the MIR equivalent of a SystemVerilog `always` block:

```rust
pub struct Process {
    pub id: ProcessId,
    pub kind: ProcessKind,
    pub sensitivity: SensitivityList,
    pub body: Block,
    pub span: Option<SourceSpan>,
}

pub enum ProcessKind {
    Sequential,      // always_ff
    Combinational,   // always_comb
    General,         // always (mixed)
    Async,           // NCL async process
}

pub enum SensitivityList {
    Edge(Vec<EdgeSensitivity>),
    Level(Vec<LValue>),
    Always,
}
```

A `Sequential` process has an `Edge` sensitivity list — it fires on `Rising` or `Falling` edges of specific signals (typically clocks). A `Combinational` process has either a `Level` sensitivity list (re-evaluate when any listed signal changes) or `Always` (re-evaluate on any input change).

The distinction between `Sequential` and `Combinational` is crucial for SIR conversion. Sequential processes produce `FlipFlop` nodes in SIR — they go in the `sequential_nodes` list and are evaluated only on clock edges. Combinational processes produce `BinaryOp`, `Mux`, `Concat` nodes — they go in `combinational_nodes` and are evaluated every simulation step in topological order.

**Module instances preserve hierarchy.** A `ModuleInstance` references another module by ID and connects ports:

```rust
pub struct ModuleInstance {
    pub name: String,
    pub module_name: String,
    pub module_id: ModuleId,
    pub port_connections: Vec<PortConnection>,
    pub parameter_overrides: Vec<ParameterOverride>,
    pub span: Option<SourceSpan>,
}
```

MIR preserves hierarchy — a top module that instantiates a FIFO still has the FIFO as a separate module with its own ports and processes. SIR flattens this: the FIFO's logic is inlined into the top module with prefixed signal names. LIR also flattens, but preserves word-level operations. SystemVerilog generation preserves hierarchy (each module becomes a separate `module` declaration with `instantiation` statements).

**Continuous assignments** handle simple wiring:

```rust
pub struct ContinuousAssign {
    pub lhs: LValue,
    pub rhs: Expression,
    pub span: Option<SourceSpan>,
}
```

These become `assign` statements in SystemVerilog, constant-propagation targets in LIR, and direct signal connections in SIR.

**What you can count in MIR.** At the MIR level, you can count flip-flops (sequential processes with edge sensitivity), trace timing paths (combinational dependency chains through processes), identify clock domain crossings (signals used in processes with different clock sensitivities), and verify port widths. MIR is concrete enough for all of these analyses but abstract enough that it doesn't commit to a specific technology or gate library. It's architecture-independent RTL.

---

## LIR: Words Before Gates

LIR is skalp's technology mapping representation. It sits between MIR (behavioral RTL) and the backend that emits SystemVerilog netlists or FPGA bitstreams. The key design decision in LIR: preserve word-level operations instead of eagerly decomposing to individual gates.

Why word-level? Technology libraries have compound cells. A standard cell library doesn't just have NAND2 and NOR2 — it has 8-bit adders (ADDER8), 4:1 multiplexers (MUX4), AOI22 (AND-OR-Invert), OAI33, carry-lookahead blocks, and multi-bit flip-flops. If the IR eagerly decomposes a 32-bit add into 32 full adders and a carry chain, the technology mapper has to pattern-match those 100+ gates back into an ADDER32 cell. That's NP-hard in general and fragile in practice. If the IR preserves "32-bit add" as a single operation, the mapper can directly select the optimal library cell.

The core types:

```rust
pub struct Lir {
    pub name: String,
    pub nodes: Vec<LirNode>,
    pub signals: Vec<LirSignal>,
    pub inputs: Vec<LirSignalId>,
    pub outputs: Vec<LirSignalId>,
    pub clocks: Vec<LirSignalId>,
    pub resets: Vec<LirSignalId>,
    pub detection_signals: Vec<LirSignalId>,
    pub is_ncl: bool,
    pub module_safety_info: Option<LirSafetyInfo>,
    signal_map: IndexMap<String, LirSignalId>,
}
```

Each `LirNode` is an operation with typed inputs, an output, and optional clock/reset for sequential elements:

```rust
pub struct LirNode {
    pub id: LirNodeId,
    pub op: LirOp,
    pub inputs: Vec<LirSignalId>,
    pub output: LirSignalId,
    pub path: String,
    pub clock: Option<LirSignalId>,
    pub reset: Option<LirSignalId>,
}
```

The `path` field preserves hierarchy information — a node at `path = "fifo.wr_logic.add_0"` came from an adder in the write logic of a FIFO instance. This survives through LIR so that timing reports and area breakdowns can reference the original design hierarchy.

**The PrimitiveType enum is the full taxonomy of hardware primitives:**

```rust
pub enum PrimitiveType {
    // Combinational logic
    And, Or, Nand, Nor, Xor, Xnor, Inv, Buf, Tribuf,
    Mux2, Mux4, MuxN,

    // Sequential logic
    DffP, DffN, DffNeg, DffE, DffAR, DffAS, DffScan,
    Dlatch, SRlatch,

    // Arithmetic
    HalfAdder, FullAdder, CarryCell, CompBit,

    // Floating-point
    Fp32Add, Fp32Sub, Fp32Mul, Fp32Div,
    Fp32Lt, Fp32Gt, Fp32Le, Fp32Ge,

    // Memory
    MemCell, RegCell,

    // Special
    ClkBuf, Constant,

    // FPGA LUTs
    Lut4, Lut6,

    // Power infrastructure
    LevelShifter, IsolationCell, RetentionDff, PowerSwitch, AlwaysOnBuf,

    // NCL threshold gates
    Th12, Th22, Th13, Th23, Th33,
    Th14, Th24, Th34, Th44,
    Thmn, ThmnW,
    NclCompletion,
}
```

This taxonomy is intentionally broad. It covers:

- **Standard digital logic** (And through MuxN) — the basics that every technology library provides.
- **Sequential elements** (DffP through SRlatch) — six variants of D flip-flop (positive edge, negative edge, with enable, with async reset, with async set, with scan chain) plus latches.
- **Arithmetic** — half adders, full adders, carry cells, and comparator bits. These are the building blocks that technology mappers compose into multi-bit arithmetic units.
- **FPGA primitives** — LUT4 and LUT6 directly target Xilinx/Intel FPGA architectures where everything maps to lookup tables.
- **Power infrastructure** — level shifters, isolation cells, retention flip-flops, power switches, always-on buffers. These are ISO 26262 and low-power design essentials that most EDA IRs treat as out-of-band annotations. LIR makes them first-class primitives.
- **NCL threshold gates** — Null Convention Logic for asynchronous design. TH12 is a 2-input threshold-1 gate (an OR), TH22 is threshold-2 (an AND), TH23 is a 3-input threshold-2 gate, and so on. These have no direct equivalent in synchronous logic — they're the fundamental building blocks of delay-insensitive computation.

**The LirOp enum preserves word-level semantics:**

```rust
pub enum LirOp {
    // Arithmetic (word-level)
    Add, Sub, Mul,

    // Bitwise logic (word-level)
    And, Or, Xor, Not, Nand, Nor, Buf,

    // Comparison (word-level)
    Eq, Ne, Lt, Le, Gt, Ge,
    Slt, Sle, Sgt, Sge,  // Signed comparisons

    // Multiplexing
    Mux2, MuxN,

    // Shift (word-level)
    Shl, Shr, Sar, Rol, Ror,

    // Reduction
    RedAnd, RedOr, RedXor,

    // Bit manipulation
    Concat, BitSelect, RangeSelect, ZeroExtend, SignExtend,

    // Sequential
    Reg, Latch, MemRead, MemWrite,

    // Special
    Constant, Buffer, Tristate,

    // NCL operations
    Th12, Th22, NclEncode, NclDecode,
    NclAnd, NclOr, NclXor, NclNot,
    NclAdd, NclSub, NclMul,
    NclLt, NclEq,
    NclShl, NclShr,
    NclMux2, NclReg, NclComplete, NclNull,
}
```

Notice the parallel structure: `Add` and `NclAdd`, `And` and `NclAnd`, `Reg` and `NclReg`. The NCL variants operate on dual-rail encoded signals and follow NULL convention semantics — they produce NULL outputs when any input is NULL, and produce a valid DATA output only when all inputs have valid DATA values. This is fundamentally different from synchronous logic where outputs are always valid.

**Each LirOp knows its output width and input count:**

```rust
impl LirOp {
    pub fn output_width(&self) -> u32 { ... }
    pub fn input_count(&self) -> usize { ... }
    pub fn is_sequential(&self) -> bool { ... }
}
```

The technology mapper uses these methods to select appropriate library cells. A `LirOp::Add` with 32-bit inputs might map to four ADDER8 cells with carry chains, or one ADDER32 if the library has it, or a tree of full adders for a library that only has single-bit cells. The word-level preservation in LIR means the mapper has full information about the operation's semantics and can make the optimal choice.

**Safety information propagates through LIR:**

```rust
pub struct LirSafetyInfo {
    pub goal_name: Option<String>,
    pub mechanism_name: Option<String>,
    pub is_sm_of_sm: bool,
    pub protected_sm_name: Option<String>,
    pub is_boot_time_only: bool,
}
```

A primitive that implements a safety mechanism (say, a parity checker for a register bank) carries `mechanism_name = Some("reg_bank_parity")` and `goal_name = Some("SG_RegBank")`. This propagates from the HIR `DetectionConfig` through MIR's `SafetyContext` to LIR's `LirSafetyInfo`. The fault simulator uses this to classify faults as "detected by mechanism X" vs "undetected" — the foundation of ISO 26262 FMEDA (Failure Modes Effects and Diagnostic Analysis).

---

## SIR: Shaped for the GPU

SIR is the simulation IR — the representation that [SharedCodegen](/blog/gpu-accelerated-simulation/#the-sharedcodegen-architecture) consumes to produce Metal compute shaders and compiled C++. Where MIR preserves hierarchy and process semantics for SystemVerilog generation, SIR flattens everything into a shape that maps directly to GPU execution.

The `SirModule` struct:

```rust
pub struct SirModule {
    pub name: String,
    pub inputs: Vec<SirPort>,
    pub outputs: Vec<SirPort>,
    pub signals: Vec<SirSignal>,
    pub combinational_nodes: Vec<SirNode>,
    pub sequential_nodes: Vec<SirNode>,
    pub state_elements: HashMap<String, StateElement>,
    pub clock_domains: HashMap<String, ClockDomain>,
    pub sorted_combinational_node_ids: Vec<usize>,
    pub pipeline_config: Option<PipelineConfig>,
    pub span: Option<SourceSpan>,
    pub name_registry: NameRegistry,
}
```

**Key differences from MIR:**

**No hierarchy.** MIR has `ModuleInstance` — a reference to another module that must be elaborated separately. SIR has no instances. Everything is flat. A design with a top module instantiating a UART, which instantiates a shift register, becomes one `SirModule` with all signals and nodes inlined. The `name_registry` records the original hierarchy (`uart.shift_reg.data`) so the testbench can reference signals by their hierarchical path.

**Explicit combinational/sequential separation.** MIR has `Process` with a `ProcessKind` that might be `Sequential` or `Combinational`. SIR has two separate node lists: `combinational_nodes` and `sequential_nodes`. The GPU generates different kernels for each — the combinational kernel runs every step, the sequential kernel runs only on clock edges. Having separate lists means the code generator iterates one list per kernel, with no filtering.

**Pre-computed topological order.** MIR processes don't have an explicit evaluation order — the simulator must schedule them based on sensitivity lists and signal changes. SIR pre-computes the evaluation order using Kahn's algorithm and stores it in `sorted_combinational_node_ids`. Every simulation step walks this list in order. No event queue, no priority scheduling, no dependency checking at runtime.

**Combinational cone extraction.** SIR can extract `CombinationalCone` structures — groups of combinational nodes that form independent subgraphs. Independent cones have no data dependencies between them and could execute on separate GPU cores. Currently skalp generates a single combinational kernel, but the cone extraction infrastructure exists for future multi-cone parallelism.

**The SirNode and SirNodeKind types:**

```rust
pub struct SirNode {
    pub id: usize,
    pub kind: SirNodeKind,
    pub inputs: Vec<SignalRef>,
    pub outputs: Vec<SignalRef>,
    pub clock_domain: Option<String>,
    pub impl_style_hint: ImplStyleHint,
    pub span: Option<SourceSpan>,
}

pub enum SirNodeKind {
    BinaryOp(BinaryOperation),
    UnaryOp(UnaryOperation),
    Mux,
    ParallelMux { num_cases, match_values, result_width },
    Concat,
    Slice { start, end },
    Constant { value, width },
    SignalRef { signal },
    FlipFlop { clock_edge },
    Latch { enable },
    Memory { depth, width },
    ArrayRead,
    ArrayWrite,
    ClockGate,
    Reset,
}
```

Each `SirNodeKind` maps directly to a code generation pattern. `BinaryOp(Add)` becomes `a + b` in the shader. `Mux` becomes `sel ? a : b`. `FlipFlop` becomes a register read in the sequential kernel and a register write conditioned on clock edge. `ParallelMux` becomes a chain of `if/else if` conditions matching against `match_values`. The code generator has no interpretation step — each node kind has a fixed code template, and SharedCodegen fills in the operands.

**Topological sorting with Kahn's algorithm.** The `finalize_topological_order()` method builds the sorted evaluation order:

1. Compute in-degree for each combinational node (how many of its inputs come from other combinational nodes).
2. Enqueue all zero in-degree nodes (their inputs are primary inputs, registers, or constants).
3. Process the queue: dequeue a node, add to sorted order, decrement in-degree of all nodes that consume this node's outputs.
4. Repeat until the queue is empty.

If all nodes are processed, the sort succeeds and `sorted_combinational_node_ids` contains the evaluation order. If some nodes remain (in-degree never reached zero), there's a combinational cycle. The algorithm logs a warning and appends unsorted nodes as a fallback — the simulation runs but may not converge for signals in the cycle.

The sort runs once at SIR construction time. Cost: O(N + E) for N nodes and E edges. For a typical 10K-node design, this takes microseconds. The result is a flat array of node IDs that every simulation step walks sequentially — the simplest possible dispatch pattern for a GPU.

For the full details on how SharedCodegen turns SIR into Metal shaders and compiled C++, how the three compute kernels work, and how fault simulation runs one-thread-per-fault, see the [GPU-Accelerated RTL Simulation](/blog/gpu-accelerated-simulation/) post.

---

## HIR → MIR: Lowering Intent to RTL

The `HirToMir` transformer converts the designer's intent into cycle-accurate RTL. This is the most complex lowering step because it must make implementation decisions that HIR deliberately leaves open.

```rust
pub struct HirToMir<'hir> {
    entity_map: IndexMap<hir::EntityId, ModuleId>,
    port_map: IndexMap<hir::PortId, PortId>,
    flattened_ports: IndexMap<hir::PortId, Vec<FlattenedField>>,
    signal_map: IndexMap<hir::SignalId, SignalId>,
    flattened_signals: IndexMap<hir::SignalId, Vec<FlattenedField>>,
    // ... many tracking fields
}
```

What happens in this lowering:

**Type flattening.** Struct-typed ports and signals become multiple flat signals. A port of type `struct { valid: bit, data: bit<32>, tag: bit<4> }` becomes three MIR ports: `port_valid` (1 bit), `port_data` (32 bits), `port_tag` (4 bits). The `flattened_ports` map records which flat ports came from which HIR struct port, so error messages can reference the original name. Enum types become integer-encoded signals with a width sufficient to hold the largest variant discriminant.

**Expression lowering.** HIR expressions are richer than MIR — they include match expressions with pattern matching, method calls, associated constants, and field access on structs. The lowering converts:

- `match` expressions → `case` statements with explicit value comparisons
- Struct field access → bit-range selection on the flattened signal
- Enum variant references → integer constants
- Method calls → inlined function bodies
- Associated constants → literal values

**Process generation.** HIR `HirEventBlock` entries (triggered by clock edges) become MIR `Process` entries with `ProcessKind::Sequential` and `EdgeSensitivity`. HIR combinational assignments become MIR processes with `ProcessKind::Combinational`. The key transformation: HIR doesn't distinguish between combinational and sequential assignments — it uses `HirAssignmentType::NonBlocking` vs `HirAssignmentType::Blocking`. The lowering examines the context (is this assignment inside a clock-triggered event block?) to determine the MIR `ProcessKind`.

**Function inlining.** skalp functions can be called from hardware descriptions. At the MIR level, these must be inlined — there's no function call mechanism in hardware. The lowering inlines up to `MAX_INLINE_CALL_COUNT = 5` levels deep. Beyond that depth, it reports an error (likely infinite recursion or unreasonably deep call chains).

**Generic monomorphization.** A generic entity `Fifo<WIDTH, DEPTH>` instantiated as `Fifo<32, 8>` gets a concrete MIR module with all generic parameters replaced. The monomorphizer substitutes every occurrence of `WIDTH` with `32` and `DEPTH` with `8`, resolves parameterized types (`Bit(BitParam { param: "WIDTH" })` → `Bit(32)`), and evaluates constant expressions involving generic parameters.

**Pipeline register insertion.** If the HIR entity has a `PipelineConfig`, the lowering inserts pipeline registers between stages. A `#[pipeline(stages=3)]` annotation becomes three sets of pipeline registers with stage enables and valid signals. This is where the single annotation "explodes" into dozens of flip-flops and mux chains — a `32-bit` datapath with 3 pipeline stages generates 96 pipeline register bits plus control logic.

What's lost: the fact that those flip-flops are pipeline registers (as opposed to design registers), the stage structure, the hazard detection intent. MIR has flip-flops and muxes, but it doesn't know they form a pipeline. If you need to analyze pipeline behavior, you must do it in HIR.

---

## MIR → LIR: RTL to Primitives

The `MirToLirTransform` converts cycle-accurate RTL into word-level primitives suitable for technology mapping.

```rust
pub struct MirToLirTransform {
    lir: Lir,
    port_to_signal: IndexMap<PortId, LirSignalId>,
    signal_to_lir_signal: IndexMap<SignalId, LirSignalId>,
    variable_to_signal: IndexMap<VariableId, LirSignalId>,
    port_widths: IndexMap<PortId, u32>,
    signal_widths: IndexMap<SignalId, u32>,
    hierarchy_path: String,
    warnings: Vec<String>,
    clock_signals: Vec<LirSignalId>,
    reset_signals: Vec<LirSignalId>,
    // ...
}
```

The transformation runs in phases:

1. **Create port signals.** Each MIR port becomes a LIR signal with the same width. Input ports are marked as inputs in the LIR, output ports as outputs.

2. **Create internal signals.** MIR signals become LIR signals. Width information is extracted from the `DataType` and stored in `signal_widths`.

3. **Create variable signals.** MIR variables (from `let` bindings) become LIR signals. This was added as a fix (BUG #150) — variables were originally ignored, causing missing drivers in the LIR graph.

4. **Transform continuous assignments.** Each `ContinuousAssign` becomes a chain of LIR nodes. A simple assignment like `assign out = a & b` becomes one `LirOp::And` node. A complex assignment like `assign out = sel ? (a + b) : (c & d)` becomes three nodes: Add, And, Mux2.

5. **Transform processes.** This is the bulk of the work. Each MIR `Process` is walked statement by statement:
   - `if` statements become `Mux2` chains
   - `case` statements become `MuxN` priority encoders
   - Assignments generate the LIR operation corresponding to the RHS expression
   - Sequential processes wrap their outputs in `Reg` nodes with clock and reset connections

6. **Populate clock and reset nets.** Clock and reset signals are identified and stored separately for downstream tools.

7. **NCL expansion** (for async designs). If the module is marked `is_async`, synchronous operations are expanded into NCL equivalents: `Add` becomes `NclAdd`, `And` becomes `NclAnd`, `Reg` becomes `NclReg`, and completion detection logic is inserted. This is a whole-module transformation — every operation gets an NCL counterpart with dual-rail encoding and NULL propagation semantics.

**Safety context propagation.** If the MIR module has a `SafetyContext`, it's converted to `LirSafetyInfo` and attached to the LIR:

```rust
fn safety_context_to_lir_info(ctx: &SafetyContext) -> LirSafetyInfo {
    LirSafetyInfo {
        goal_name: ctx.implementing_goal.clone(),
        mechanism_name: ctx.mechanism_name.clone(),
        is_sm_of_sm: false,
        protected_sm_name: None,
        is_boot_time_only: false,
    }
}
```

**Hierarchical lowering.** The `lower_mir_hierarchical()` function handles multi-module designs. It elaborates each module instance, producing a `HierarchicalMirToLirResult` with per-instance LIR results and port connection information. The `flatten()` method on this result produces a single flat LIR by inlining all instances with prefixed signal names — similar to SIR flattening, but at the LIR level.

```rust
pub struct HierarchicalMirToLirResult {
    pub instances: IndexMap<String, InstanceLirResult>,
    pub top_module: String,
    pub hierarchy: IndexMap<String, Vec<String>>,
}
```

The word-level philosophy shows up clearly in how expressions are transformed. A MIR expression `a + b` where `a` and `b` are 32-bit signals becomes one `LirOp::Add` node with two 32-bit input signals and one 32-bit output signal. Not 32 full adders. Not a ripple-carry chain. Just `Add`. The technology mapper downstream decides how to implement that add — carry-lookahead, carry-select, or ripple-carry — based on the target library and timing constraints. LIR's job is to preserve the operation's semantics and width, not to make implementation choices.

---

## MIR → SIR: RTL to Simulation

The `convert_mir_to_sir_with_hierarchy()` function is the entry point for behavioral simulation. It takes the full MIR design and produces a flat, topologically sorted `SirModule` ready for GPU dispatch.

```rust
pub fn convert_mir_to_sir_with_hierarchy(
    mir: &Mir,
    top_module: &Module,
) -> SirModule
```

The converter struct tracks the mapping between MIR and SIR namespaces:

```rust
struct MirToSirConverter<'a> {
    sir: &'a mut SirModule,
    mir: &'a Module,
    mir_design: &'a Mir,
    node_counter: usize,
    signal_map: HashMap<String, String>,
    elaborated_instances: HashSet<String>,
    instance_parent_module_ids: HashMap<String, ModuleId>,
    mir_to_internal_name: HashMap<String, String>,
    sequential_defaults: HashMap<String, usize>,
    // ...
}
```

The conversion runs in eight phases:

1. **`convert_ports()`** — MIR ports become SIR ports. Names are registered in the `NameRegistry` and the `signal_map` is populated.

2. **`convert_signals()`** — MIR signals become SIR signals. Each signal is checked for sequential behavior (is it assigned in a `ProcessKind::Sequential` process?) and marked with `is_state`. Signals assigned in sequential processes generate `StateElement` entries.

3. **`convert_variables()`** — MIR variables become SIR signals. Variables are internal to a process in MIR but need to be visible as signals in SIR for the code generator to reference them.

4. **`flatten_instances()`** — The converter walks each `ModuleInstance`, looks up the referenced module in `mir_design`, and recursively converts it. All child ports, signals, and logic are prefixed with the instance path (e.g., `uart_tx__shift_reg__data` — double underscore to avoid collisions). Circular instantiation is detected via `elaborated_instances` and reported as an error.

5. **`convert_logic()`** — The main work. Dispatches to three sub-converters:
   - `convert_continuous_assign()` — simple wiring becomes signal reference nodes
   - `convert_combinational_block()` — `always_comb` processes become trees of `BinaryOp`, `UnaryOp`, `Mux`, `Concat` nodes in `combinational_nodes`
   - `convert_sequential_block()` — `always_ff` processes become `FlipFlop` nodes in `sequential_nodes`, with the combinational input logic placed in `combinational_nodes`

6. **`extract_clock_domains()`** — Clock signals are identified from the MIR's `ClockDomain` declarations and from the `EdgeSensitivity` lists in sequential processes.

7. **`insert_pipeline_registers()`** — If a `PipelineConfig` exists, additional flip-flop nodes and control signals are generated.

8. **`finalize_topological_order()`** — Kahn's algorithm runs on `combinational_nodes` to produce `sorted_combinational_node_ids`.

**Why MIR → SIR instead of LIR → SIR for behavioral simulation?** It's about GPU efficiency. MIR preserves multi-bit semantics: a 32-bit add is one `BinaryOp::Add` node. The code generator turns this into `uint a + uint b` in the Metal shader — a single ALU instruction. If we went through LIR first, the 32-bit add would decompose into word-level primitives (still one `LirOp::Add`), but the LIR representation adds overhead: separate signal IDs for every intermediate wire, separate nodes for operations that MIR keeps in a single expression tree. Going directly from MIR preserves the expression structure that SharedCodegen exploits for compact code generation.

More importantly, MIR's type system maps naturally to GPU types. A `DataType::Bit(32)` becomes `uint` in Metal. A `DataType::Bit(48)` becomes `uint2`. A `DataType::Float32` becomes `uint` with bitcast semantics. LIR doesn't have this mapping — its signals are just `LirSignal` with a width, and the type information that drives GPU type selection would need to be reconstructed.

**The sequential block conversion** deserves special attention. When the converter encounters a `ProcessKind::Sequential` process, it must separate the combinational input logic from the flip-flop:

```
MIR:                              SIR:
always_ff @(posedge clk) {        combinational_nodes:
  if (enable) {                     [Mux: sel=enable, a=old_q, b=d] → mux_out
    q <= d;                       sequential_nodes:
  }                                 [FlipFlop: input=mux_out] → q
}
```

The `if (enable)` becomes a `Mux` in `combinational_nodes` — it selects between the flip-flop's current value and the new data based on the enable signal. The flip-flop itself goes in `sequential_nodes` and takes the mux output as input. This separation means the combinational kernel evaluates the mux (computing what the flip-flop would sample), and the sequential kernel just copies the mux output to the register on a clock edge.

---

## LIR → SIR: Gates to Fault Simulation

For gate-level fault simulation, the design takes the long path: HIR → MIR → LIR → SIR. This produces a different kind of SIR — not the behavioral SIR with `BinaryOp` and `Mux` nodes, but a flat list of `GpuPrimitive` structs that the fault simulator dispatches one-thread-per-fault.

The fault simulator's SIR doesn't use `SirModule` directly. Instead, the LIR is converted to a flat primitive list:

```rust
#[repr(C)]
struct GpuPrimitive {
    ptype: u32,       // PrimitiveType encoding
    inputs: [u32; 4], // Input net IDs
    num_inputs: u32,
    output: u32,      // Output net ID
    _pad: [u32; 2],
}
```

Each `GpuPrimitive` corresponds to one `LirNode` — an AND gate, an OR gate, a flip-flop, a mux. The `ptype` field encodes the `PrimitiveType` as a u32 for GPU consumption. The `inputs` array holds net IDs (indices into the signal state array), and `output` is the net ID where the result is written.

The conversion from LIR to this flat form is straightforward: walk the LIR nodes in order, encode each node's `LirOp` as a `PrimitiveType`, map each `LirSignalId` to a net index, and pack into the `GpuPrimitive` struct. The ordering matters — primitives must be in topological order so that each gate reads valid inputs when it evaluates.

The GPU fault simulation kernel then evaluates all primitives in order for each cycle, with one twist: after evaluating a primitive, it checks whether this primitive is the fault target for the current thread. If so, it applies the fault (stuck-at-0, stuck-at-1, bit-flip, or transient) to the output value before writing it to the signal array. Each thread has its own copy of the signal array, so faults don't interfere between threads.

Why go through LIR for fault simulation instead of using behavioral SIR directly? Because faults are physical phenomena — a stuck-at-0 happens at a specific gate output, not at a behavioral level. A 32-bit adder in behavioral SIR is one node, but a stuck-at-0 on bit 7 of the adder's output requires knowing that bit 7 exists as a separate wire. LIR decomposes the adder into individual primitives where each output is a distinct net that can be independently faulted. This gate-level granularity is what ISO 26262 fault simulation requires — the standard specifies fault models at the gate and transistor level, not at the behavioral level.

The performance advantage of GPU fault simulation is dramatic. Each fault is independent — thread 0 injects fault 0 and simulates N cycles, thread 1 injects fault 1, and so on. No inter-thread communication, no shared mutable state, no synchronization. On an M1 Max with 32 GPU cores, this runs at ~10M fault simulations per second. See the [GPU simulation post](/blog/gpu-accelerated-simulation/#fault-simulation-embarrassingly-parallel) for full details on performance and kernel implementation.

---

## Safety Through the Pipeline

ISO 26262 safety information must survive all four IRs. A safety mechanism annotated in the source must still be identifiable at the gate level, where fault simulation determines whether it actually detects the faults it claims to detect. Here's how safety information flows through each lowering:

**HIR: Declaration.** The designer annotates ports and signals with `DetectionConfig`:

```rust
pub struct DetectionConfig {
    pub mode: DetectionMode,          // Continuous, Windowed, etc.
    pub detection_signal: Option<String>,
    // ...
}
```

A port marked `#[detection(mode = "continuous")]` declares that this signal is a safety mechanism output. The HIR preserves the designer's intent — what kind of detection, what it protects, what the monitoring window is.

**MIR: Contextualization.** The module-level `SafetyContext` aggregates all safety annotations:

```rust
pub struct SafetyContext {
    pub implementing_goal: Option<String>,
    pub is_sm_signal: bool,
    pub mechanism_name: Option<String>,
    pub dc_override: Option<f64>,
    pub lc_override: Option<f64>,
}
```

At MIR level, safety information is attached to the module, not individual signals. The `implementing_goal` links to an ISO 26262 safety goal (e.g., "SG_01: Prevent unintended acceleration"). The `mechanism_name` identifies which safety mechanism this module implements (e.g., "SM_WDT: Watchdog Timer").

**LIR: Per-primitive tagging.** Each `LirNode` that's part of a safety mechanism carries `LirSafetyInfo`:

```rust
pub struct LirSafetyInfo {
    pub goal_name: Option<String>,
    pub mechanism_name: Option<String>,
    pub is_sm_of_sm: bool,
    pub protected_sm_name: Option<String>,
    pub is_boot_time_only: bool,
}
```

The `is_sm_of_sm` field handles the recursive case — a watchdog timer (safety mechanism) might itself be protected by a timeout checker (safety mechanism of a safety mechanism). `protected_sm_name` identifies which mechanism is being protected. `is_boot_time_only` flags mechanisms that only operate during startup (like BIST — Built-In Self Test).

Each primitive also has a `base_fit()` method on its `PrimitiveType` that returns the base Failure-In-Time rate for that primitive type. A `DffP` has a higher FIT rate than an `And` gate because flip-flops are more complex transistor structures. These base rates are scaled by environmental factors (temperature, voltage) from the `FitOverrides` configuration.

**SIR: Detection signal tracking.** In the SIR used for fault simulation, detection signals are tracked by their net IDs. The fault simulation kernel checks these specific signals after each simulation cycle — if any detection signal differs from the golden (fault-free) value, the fault is detected.

The flow: a designer writes `#[detection(mode = "continuous")]` on a signal → HIR records the `DetectionConfig` → MIR propagates it to `SafetyContext` → LIR tags individual primitives with `LirSafetyInfo` → SIR's fault simulator monitors the detection signal net → fault campaign results report which faults each mechanism detects → diagnostic coverage = detected faults / total faults.

The end result is an FMEDA (Failure Modes Effects and Diagnostic Analysis) that traces from each gate-level fault through the detection mechanism back to the safety goal — a complete safety argument from silicon to specification, with every step automated by the compiler pipeline.

---

## Why Four IRs

Every IR serves a different master.

**HIR serves the designer.** It preserves intent, generics, and annotations. The designer writes `#[pipeline(stages=3)]` and expects the tool to handle register insertion, stage enables, and hazard detection. HIR is where the designer's mental model lives. If you tried to serve the designer with MIR, they'd have to manually instantiate pipeline registers. If you tried to serve them with LIR, they'd be writing gate-level netlists. If you tried to serve them with SIR, they'd be specifying topological sort orders.

**MIR serves the synthesis and verification engineer.** It's cycle-accurate, architecture-independent, and inspectable. You can count flip-flops, trace timing paths, check clock domain crossings, and generate SystemVerilog that any RTL tool can consume. MIR is the common ground between skalp and the rest of the EDA ecosystem. If you tried to serve the verification engineer with HIR, they couldn't count flip-flops (some are still annotations). If you tried with LIR, they'd be looking at gate-level netlists before synthesis. If you tried with SIR, they'd lose the module hierarchy that makes designs comprehensible.

**LIR serves the technology mapper.** It preserves word-level operations so the mapper can select optimal library cells without pattern-matching decomposed gates back into compound operations. LIR is where the rubber meets the silicon — it's the last representation before physical implementation. If you tried to serve the mapper with MIR, it would need to decompose behavioral processes into gates itself. If you tried with HIR, it would need to do the entire compilation. If you tried with SIR, it doesn't have the gate-level primitives needed for technology libraries.

**SIR serves the GPU.** It's flat, sorted, and pre-scheduled. No hierarchy to elaborate at runtime, no processes to classify, no dependencies to resolve. The GPU walks a sorted array of nodes and evaluates them. If you tried to serve the GPU with MIR, every simulation step would need to elaborate hierarchy, classify processes, and schedule evaluation dynamically. If you tried with HIR, you'd need the entire compiler in the GPU runtime. If you tried with LIR, behavioral simulation would decompose multi-bit operations into gate-level primitives, wasting GPU ALUs on single-bit operations when they can handle 32-bit operations natively.

The cost of four IRs is complexity in the compiler — four representations means four sets of data structures, four sets of lowering passes, and four sets of tests. The benefit is optimal representation at each stage. Each IR makes its consumer's job trivial: the SystemVerilog emitter walks MIR and prints. The technology mapper walks LIR and selects cells. The GPU code generator walks SIR and emits shaders. None of them need to do work that belongs to a different stage.

A single IR would be a compromise. It would be too concrete for the designer (no generics, no pipeline annotations), too abstract for the mapper (no gate-level primitives), too hierarchical for the GPU (no flat sorted nodes), or too flat for the verification engineer (no module structure). Every user of the IR would spend effort adapting the representation to their needs — work that the compiler should do once, correctly, at lowering time.

Four IRs is the minimum. You could argue for more — a separate IR for physical placement, another for power analysis, another for formal verification. skalp may grow more IRs over time. But four is the right starting point because it covers the four fundamentally different consumers: the human designer, the synthesis tool, the technology mapper, and the simulation engine. Each gets exactly the representation it needs, and the compiler handles all the transformations between them.
