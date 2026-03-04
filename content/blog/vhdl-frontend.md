---
title: "VHDL as a First-Class Frontend: Thirty Years of IP, Modern Tooling"
date: 2026-03-03
summary: "skalp now accepts VHDL directly — including VHDL-2019 interfaces and views that no free tool supports. Your existing designs get CDC analysis, formal verification, gate-level simulation, and Rust async testbenches without rewriting a single line."
tags: ["skalp", "vhdl", "hdl", "compiler", "hardware", "vhdl-2019"]
ShowToc: true
---

VHDL engineers have decades of IP locked into vendor-specific toolchains. A power converter controller validated over three product generations. A safety-critical motor drive that passed ISO 26262 audit. An AXI interconnect that took two engineers eighteen months to verify. These designs work. They're production-proven. And they're stuck.

"Stuck" doesn't mean broken — it means limited. Modern verification and analysis capabilities exist, but accessing them means either buying commercial tools that cost six figures annually (Spyglass for CDC, JasperGold for formal, Tessent for fault simulation) or rewriting the design in whatever language the open-source tool du jour supports. Neither option is practical for a team with a million lines of validated VHDL.

skalp now accepts VHDL directly. Not as a compatibility shim or a limited subset importer, but as a full frontend that parses VHDL — including VHDL-2019 features that no free tool and few commercial tools support — and lowers it to the same intermediate representation used by skalp's native language. Once there, the VHDL design gets everything the skalp backend provides: CDC analysis, formal verification, gate-level simulation, GPU-accelerated fault injection, cross-language codegen, and Rust async testbenches. Without rewriting a single line.

---

## The Architecture: Two Frontends, One Backend

The VHDL frontend is not a translator that converts VHDL to skalp syntax. It's a proper compiler frontend — lexer, parser, and HIR lowering stage — that maps VHDL constructs directly to skalp's High-level IR. Once at HIR, the design enters the shared pipeline and becomes indistinguishable from a skalp-native design.

```
                    ┌─────────────────────┐
                    │   skalp Source (.sk) │
                    └──────────┬──────────┘
                               │ parse + resolve
                               ▼
                    ┌──────────────────────┐
                    │         HIR          │
                    │  (intent, types,     │◄──────────────┐
                    │   safety attrs)      │               │
                    └──────────┬───────────┘               │
                               │                           │ lex + parse + lower
                               │              ┌────────────┴──────────┐
                               │              │   VHDL Source (.vhd)  │
                               │              └───────────────────────┘
                               │ lower intent → RTL
                               ▼
                    ┌──────────────────────┐
                    │         MIR          │
                    │  (cycle-accurate     │
                    │   RTL, hierarchy)    │
                    └──┬───────────────┬───┘
                       │               │
    ┌──────────────────┘               └──────────────────┐
    │ synthesis path                    simulation path    │
    ▼                                                     ▼
  ┌──────────────────┐                          ┌──────────────────┐
  │       LIR        │                          │       SIR        │
  │  (word-level     │                          │  (flat, sorted,  │
  │   primitives)    │                          │   GPU-ready)     │
  └──┬───────────┬───┘                          └──┬───────────┬───┘
     │           │                                 │           │
     ▼           ▼                                 ▼           ▼
┌─────────┐ ┌─────────┐                     ┌─────────┐ ┌─────────┐
│SystemV. │ │Bitstream│                     │ Metal   │ │Compiled │
│ (.sv)   │ │ (.bit)  │                     │ shader  │ │ C++     │
└─────────┘ └─────────┘                     └─────────┘ └─────────┘
```

The key insight is that skalp source and VHDL source enter the shared pipeline at the same level. skalp source goes through its own parser and reaches HIR, where generics, pipeline annotations, safety attributes, and clock domain specifications are preserved. VHDL source goes through a separate three-stage frontend — lex, parse, lower — and also reaches HIR. The differences:

- skalp source carries richer intent: `#[pipeline(stages=3)]`, `stream<T>`, clock domain lifetimes, requirement traceability. These are skalp-specific features that VHDL can't express.
- VHDL source carries the synthesizable subset of VHDL: entities, architectures, processes, packages, generate statements, and the VHDL-2019 features covered below.
- At HIR, both are `HirEntity` instances with ports, signals, generics, assignments, and clock domains. The backend doesn't know — or care — which language produced them.

This means a mixed design is entirely natural. A top-level module written in skalp can instantiate a VHDL entity that instantiates another skalp module. The hierarchy is resolved at MIR, where everything is cycle-accurate RTL with concrete port widths and module instances.

### The Three-Stage VHDL Pipeline

The VHDL frontend lives in its own crate (`skalp-vhdl`) and consists of three stages:

**Stage 1: Lexer.** Built on [logos](https://github.com/maciejhirsz/logos) for high-speed tokenization. VHDL is case-insensitive — `ENTITY`, `Entity`, and `entity` are the same token — so the lexer normalizes all identifiers and keywords to lowercase during tokenization. The lexer recognizes approximately 120 token variants covering VHDL keywords, operators, IEEE builtin type names (`std_logic`, `unsigned`, `signed`), VHDL-2019 keywords (`interface`, `view`), and all literal formats (integer, real, character, string, bit string with base prefixes like `X"FF"`, `O"77"`, `B"1010"`).

Non-synthesizable keywords — `wait`, `after`, `file`, `access`, `shared`, `transport`, `reject` — are recognized as tokens but rejected at parse time with a clear error: *"construct is not synthesizable; skalp targets RTL synthesis."*

**Stage 2: Parser.** A recursive descent parser using [rowan](https://github.com/rust-analyzer/rowan) for lossless syntax tree construction, producing approximately 150 syntax node types. The parser handles the full synthesizable grammar: entities, architectures, processes (with sensitivity lists), concurrent and sequential statements, type declarations (enumerations, records, arrays), generate statements, packages (including generic packages), component and entity instantiation, and VHDL-2019 interfaces and views.

Error recovery is built in — a syntax error in one declaration doesn't prevent parsing the rest of the file. This matters for IDE integration, where partial files need to produce useful syntax trees.

**Stage 3: HIR Lowering.** The largest stage at over 4,500 lines. This is where VHDL semantics are mapped to skalp's IR:

- Entity declarations become `HirEntity` with ports and generics
- Architecture bodies become implementations with signals, assignments, and event blocks
- `process(clk)` with `rising_edge(clk)` becomes an `HirEventBlock` with clock edge sensitivity
- `process(all)` (VHDL-2008) becomes a combinational block
- VHDL types map to HIR types: `std_logic` → `Logic(1)`, `unsigned(7 downto 0)` → `Nat(8)`, `signed(15 downto 0)` → `Int(16)`, records → `Struct`, enumerations → `Enum`
- Generic parameters (including VHDL-2019 type generics) are monomorphized
- Interfaces are lowered to struct types; view ports are flattened to individual directed ports
- Package types and constants are resolved and inlined

Clock and reset detection is pattern-based. The lowering stage recognizes the standard VHDL idioms:

```vhdl
if rising_edge(clk) then    -- → Clock(clk, Rising)
if falling_edge(clk) then   -- → Clock(clk, Falling)
if rst = '1' then            -- → Reset(rst, active_high)
if rst = '0' then            -- → Reset(rst, active_low)
```

This heuristic works for the vast majority of synthesizable VHDL. It's the same pattern-matching approach that Vivado and Quartus use to infer sequential logic — if the tools agree on what constitutes a clocked process, the pattern is well-established enough to rely on.

---

## VHDL-2019: Features Your Current Tools Don't Support

VHDL-2019 introduced four features that fundamentally change how reusable hardware IP is structured. These aren't cosmetic syntax changes — they're the features that VHDL engineers have been waiting for since VHDL-2008, and the reason most teams are still stuck on VHDL-93 conventions. The problem: tool support is nearly nonexistent. GHDL doesn't support them. Most commercial simulators have partial or no support. skalp supports all four.

### Interfaces

An interface groups related signals into a reusable definition. This is the VHDL equivalent of a SystemVerilog `interface`, but with proper integration into the VHDL type system:

```vhdl
interface axi_lite is
    signal awaddr  : std_logic_vector(31 downto 0);
    signal awvalid : std_logic;
    signal awready : std_logic;
    signal wdata   : std_logic_vector(31 downto 0);
    signal wstrb   : std_logic_vector(3 downto 0);
    signal wvalid  : std_logic;
    signal wready  : std_logic;
    signal bresp   : std_logic_vector(1 downto 0);
    signal bvalid  : std_logic;
    signal bready  : std_logic;
    signal araddr  : std_logic_vector(31 downto 0);
    signal arvalid : std_logic;
    signal arready : std_logic;
    signal rdata   : std_logic_vector(31 downto 0);
    signal rresp   : std_logic_vector(1 downto 0);
    signal rvalid  : std_logic;
    signal rready  : std_logic;
end interface axi_lite;
```

Without interfaces, an AXI-Lite port list requires 16 separate port declarations — each with a direction, a type, and a name that follows the AXI naming convention. Every module that connects to an AXI-Lite bus repeats these 16 declarations. Change the data width from 32 to 64 bits and you update every module manually.

With interfaces, the 16 signals are defined once. Every module that uses AXI-Lite references the interface definition. Change the data width once and it propagates everywhere.

skalp lowers interfaces to `HirType::Struct` — each signal becomes a struct field with its type preserved. The interface is a compile-time grouping mechanism that the synthesis backend never sees.

### Mode Views

Interfaces alone don't solve the direction problem. An AXI-Lite master drives `awaddr`, `awvalid`, `wdata`, `wvalid`, `araddr`, `arvalid`, `bready`, and `rready` as outputs, and reads `awready`, `wready`, `bresp`, `bvalid`, `arready`, `rdata`, `rresp`, and `rvalid` as inputs. The slave has exactly the opposite directions. Without views, you'd need two separate interface definitions that must stay manually synchronized.

Mode views define a directional perspective on an interface:

```vhdl
view axi_master of axi_lite is
    awaddr  : out;
    awvalid : out;
    awready : in;
    wdata   : out;
    wstrb   : out;
    wvalid  : out;
    wready  : in;
    bresp   : in;
    bvalid  : in;
    bready  : out;
    araddr  : out;
    arvalid : out;
    arready : in;
    rdata   : in;
    rresp   : in;
    rvalid  : in;
    rready  : out;
end view axi_master;

entity axi_register_bank is
    port (
        clk : in std_logic;
        rst : in std_logic;
        bus : view axi_master
    );
end entity;
```

The single port `bus : view axi_master` replaces 16 individual port declarations. skalp flattens this during HIR lowering — `bus` becomes 16 individual ports with correct directions (`bus_awaddr : out`, `bus_awready : in`, etc.) and correct widths. The synthesized design has flat ports; the source code has structured interfaces.

This is similar to skalp's native `protocol` type with `~` direction flipping. The difference is that VHDL views give per-field direction control (each field independently `in`, `out`, or `inout`), while skalp's `~` flips all directions at once. Both approaches solve the same problem — defining complementary interface perspectives without duplication.

### Generic Type Parameters

VHDL has had integer generics since VHDL-87 — `generic (WIDTH : positive := 8)`. But parameterizing a design over a *type* — making a FIFO that works with any element type, not just `std_logic_vector` — required either code generation or vendor-specific workarounds.

VHDL-2019 adds true type-level generics:

```vhdl
entity generic_fifo is
    generic (
        type element_type;
        DEPTH : positive := 16
    );
    port (
        clk     : in  std_logic;
        rst     : in  std_logic;
        wr_en   : in  std_logic;
        wr_data : in  element_type;
        rd_en   : in  std_logic;
        rd_data : out element_type;
        full    : out std_logic;
        empty   : out std_logic
    );
end entity;
```

The `element_type` parameter can be instantiated with any synthesizable type — `unsigned(7 downto 0)`, a record, an enumeration. skalp monomorphizes the generic during HIR lowering, producing a concrete `HirEntity` for each unique instantiation:

```vhdl
-- Instantiation: 8-bit unsigned FIFO
u_byte_fifo : entity work.generic_fifo
    generic map (
        element_type => unsigned(7 downto 0),
        DEPTH => 32
    )
    port map (
        clk => clk, rst => rst,
        wr_en => byte_wr, wr_data => byte_data,
        rd_en => byte_rd, rd_data => byte_out,
        full => byte_full, empty => byte_empty
    );

-- Instantiation: AXI transaction FIFO
u_axi_fifo : entity work.generic_fifo
    generic map (
        element_type => axi_transaction_t,
        DEPTH => 4
    )
    port map ( ... );
```

This is genuine parametric polymorphism — the same FIFO RTL works with any type, and the compiler generates specialized versions with correct widths and field layouts. No code duplication, no macros, no preprocessor.

### Generic Package Instantiation

The fourth VHDL-2019 feature is parameterized packages — packages that take type parameters and produce specialized type and constant definitions:

```vhdl
package generic_math is
    generic (type T; CONST : integer);
    type data_array is array (0 to CONST - 1) of T;
    function add(a, b : T) return T;
end package generic_math;

-- Instantiate with specific types
package byte_math is new generic_math
    generic map (T => unsigned(7 downto 0), CONST => 256);

-- Now use byte_math.data_array, byte_math.add() with concrete types
```

skalp performs type substitution during HIR lowering — every occurrence of `T` in the package is replaced with `unsigned(7 downto 0)`, and `CONST` is replaced with `256`. The resulting types and functions are concrete and carry no generic parameters.

### Why This Matters

These four features — interfaces, views, type generics, and generic packages — are the building blocks of reusable, type-safe VHDL IP. Without them, VHDL engineers resort to code generation scripts, copy-paste with manual edits, and `std_logic_vector` ports with width conventions documented in comments.

The reason teams are stuck on VHDL-93 patterns isn't lack of desire — it's lack of tool support. GHDL, the primary free VHDL simulator, doesn't implement VHDL-2019 interfaces or views. Commercial tools have partial support at best, often with restrictions (Questa supports some VHDL-2019 features; Vivado's VHDL support stops at 2008 for most constructs). skalp's VHDL frontend supports all four features, making it — as far as we can determine — the first free tool to do so.

---

## Synthesizable Subset: A Conscious Decision

skalp's VHDL frontend accepts only the synthesizable subset of VHDL. This is not a limitation imposed by implementation difficulty — it's a deliberate design choice with specific reasoning.

**What's excluded:**

- `wait` statements — simulation scheduling, not synthesizable
- `after` clauses — simulation timing (`signal <= value after 10 ns`)
- `file` declarations and I/O — file system access
- `access` types — pointers (heap allocation)
- `shared variable` — shared mutable state between processes
- `transport` and `reject` delay models — simulation-only semantics
- `force` / `release` — runtime signal override (VHDL-2008)
- `disconnect` / `guard` — guarded signal resolution
- Physical types (except time in generics) — units like `ns`, `ps`

These are rejected at parse time, not at some downstream compilation stage. When the parser encounters `wait`, the error is immediate and specific: *"'wait' is not synthesizable; skalp targets RTL synthesis. Use Rust async testbenches for simulation control."*

**Why exclude simulation constructs?** Because simulation and verification are handled by a different, better tool: the Rust async testbench ecosystem. VHDL's simulation constructs — `wait for 10 ns`, `assert`, `report`, file I/O — were designed in the 1980s for a world where the testbench and the design lived in the same language. That made sense when VHDL simulators were the only game in town.

Today, a Rust testbench gives you:

- `async`/`await` for temporal control (instead of `wait for`)
- Real data structures (vectors, hash maps, trees) without `access` types
- File I/O through Rust's standard library
- Property-based testing with `proptest` or `quickcheck`
- Parallel test execution with `tokio`
- Coverage tracking, assertions, and test organization through `#[tokio::test]`

Your design stays in VHDL. Your tests are written in Rust. The two meet at the simulation boundary, where the Rust testbench drives inputs and checks outputs through a typed API. This separation is cleaner than a mixed VHDL design/testbench, where the simulation constructs that make testbenches convenient also make it possible to accidentally write unsynthesizable logic in the design.

---

## What VHDL Designs Get For Free

Once a VHDL design passes through the frontend and reaches HIR, it has access to every tool in skalp's backend pipeline. These are capabilities that would otherwise require separate commercial tools, each with its own license, input format, and learning curve.

### CDC Analysis

Clock domain crossings — signals used in a process clocked by one clock but driven by a process clocked by a different clock — are detected automatically. A VHDL design with two clock domains:

```vhdl
architecture rtl of dual_clock is
    signal fast_data : unsigned(7 downto 0);
    signal slow_data : unsigned(7 downto 0);
begin
    process(fast_clk)
    begin
        if rising_edge(fast_clk) then
            fast_data <= input_data;
        end if;
    end process;

    process(slow_clk)
    begin
        if rising_edge(slow_clk) then
            slow_data <= fast_data;  -- CDC violation
        end if;
    end process;
end architecture;
```

skalp detects that `fast_data` is driven in the `fast_clk` domain and read in the `slow_clk` domain without a synchronizer. This is reported as a CDC violation — the same analysis that Spyglass CDC provides, but integrated into the compile step rather than requiring a separate post-synthesis lint run.

### Formal Verification

VHDL designs can be checked against properties using skalp's formal verification engine. Equivalence checking between a VHDL design and its synthesized gate-level netlist, or between two different architectures of the same entity. Property checking against user-specified invariants. These run on the MIR representation — the same cycle-accurate RTL that would be generated from skalp source.

### Gate-Level Simulation

VHDL designs can be technology-mapped through skalp's LIR pipeline and simulated at the gate level. Target libraries include `generic_asic` (generic standard cells), `ice40` (Lattice iCE40 LUT4), and `fpga_lut4` (generic 4-input LUTs). Gate-level simulation catches issues that behavioral simulation misses — setup/hold violations, glitches, timing-dependent behavior.

### GPU-Accelerated Fault Injection

For ISO 26262 safety analysis, VHDL designs get the same [GPU-accelerated fault simulation](/blog/gpu-accelerated-simulation/) that skalp-native designs do. The design is compiled through LIR to gate-level primitives, flattened to a GPU-dispatchable format, and faults are injected one-thread-per-fault on the GPU. On an M1 Max, this runs at approximately 10 million fault-cycle simulations per second — orders of magnitude faster than CPU-based fault injection.

A VHDL safety mechanism marked with standard attributes flows through the pipeline and appears in the FMEDA results. The fault simulator knows which signals are detection outputs and classifies each injected fault as detected or undetected.

### Cross-Language Codegen

This is perhaps the most practically useful capability for teams evaluating skalp: a VHDL design can be compiled and re-emitted as SystemVerilog. Or compiled and re-emitted as skalp source. The MIR representation is language-agnostic — any backend codegen target works.

VHDL → SystemVerilog translation is useful for teams that need to integrate VHDL IP into a SystemVerilog-dominant flow. The generated SystemVerilog is clean, uses modern constructs (`always_ff`, `always_comb`, `logic`), and preserves the module hierarchy.

VHDL → skalp translation is useful for incremental migration. Take an existing VHDL module, compile it, emit skalp source, then incrementally add skalp-specific features (clock domain lifetimes, stream types, safety annotations) to the generated code.

### VS Code Debugging

VHDL designs simulated through skalp's backend can be debugged in VS Code with the skalp extension — step through simulation cycles, inspect signal values, view waveforms, and set breakpoints on signal conditions. The same debugging experience available for skalp-native designs.

---

## Rust Async Testbenches for VHDL Designs

The testbench API works identically for VHDL and skalp designs. A VHDL counter:

```vhdl
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity counter is
    port (
        clk   : in  std_logic;
        rst   : in  std_logic;
        en    : in  std_logic;
        count : out unsigned(7 downto 0)
    );
end entity;

architecture rtl of counter is
    signal count_reg : unsigned(7 downto 0) := (others => '0');
begin
    process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                count_reg <= (others => '0');
            elsif en = '1' then
                count_reg <= count_reg + 1;
            end if;
        end if;
    end process;

    count <= count_reg;
end architecture;
```

Tested with a Rust async testbench:

```rust
#[tokio::test]
async fn test_counter() {
    let mut tb = Testbench::new("counter.vhd").await.unwrap();

    // Reset
    tb.set("rst", 1u32);
    tb.set("en", 0u32);
    tb.clock(2).await;

    // Release reset, enable counting
    tb.set("rst", 0u32);
    tb.set("en", 1u32);
    tb.clock(5).await;

    // Verify count reached 5
    tb.expect("count", 5u32).await;

    // Disable, count should hold
    tb.set("en", 0u32);
    tb.clock(3).await;
    tb.expect("count", 5u32).await;
}
```

The `Testbench::new("counter.vhd")` call compiles the VHDL source through the full pipeline — lex, parse, HIR lower, MIR, SIR — and sets up the simulation runtime. From that point, the API is identical regardless of source language.

**Three simulation modes through one API.** The same testbench runs in behavioral mode (MIR → SIR, fast), gate-level mode (MIR → LIR → SIR, accurate), or NCL mode (for async designs). The mode is selected at testbench construction, not in the test logic:

```rust
// Behavioral (default)
let mut tb = Testbench::new("counter.vhd").await.unwrap();

// Gate-level with iCE40 technology mapping
let mut tb = Testbench::builder("counter.vhd")
    .gate_level(Technology::Ice40)
    .build().await.unwrap();
```

The test body doesn't change. `tb.set()`, `tb.clock()`, `tb.expect()` work the same regardless of simulation level. This is the compile-once, test-many pattern — write one test, run it at three abstraction levels, and compare results.

**Coverage tracking.** The testbench runtime tracks signal toggle coverage, branch coverage (which `if`/`case` arms were taken), and state machine coverage (which states were visited) across all simulation modes. Coverage data is available programmatically:

```rust
let coverage = tb.coverage();
assert!(coverage.toggle_rate() > 0.95);
assert!(coverage.state_coverage("state_reg") == 1.0);
```

**Multi-clock support.** For VHDL designs with multiple clock domains, the testbench provides per-clock control:

```rust
tb.set_clock("fast_clk", ClockConfig::new(10)); // 10ns period
tb.set_clock("slow_clk", ClockConfig::new(100)); // 100ns period
tb.advance(500).await; // Advance 500ns — both clocks run
```

---

## What's Supported

A comprehensive list of VHDL constructs that the frontend handles:

**Design Units**
- Entity declarations with ports and generics
- Architecture bodies (multiple architectures per entity)
- Package declarations with types, constants, functions
- Package bodies with function/procedure implementations
- Library and use clauses (`library ieee; use ieee.std_logic_1164.all;`)

**Port and Signal Types**
- `std_logic` / `std_ulogic` → 1-bit logic
- `std_logic_vector(N-1 downto 0)` → N-bit logic vector
- `unsigned(N-1 downto 0)` → N-bit unsigned integer
- `signed(N-1 downto 0)` → N-bit signed integer
- `boolean` → boolean (1-bit)
- `integer` / `natural` / `positive` → width-inferred integers
- `bit` / `bit_vector` → two-state logic
- User-defined enumeration types
- Record types (structs)
- Constrained and unconstrained array types

**Processes**
- Clocked processes with `rising_edge()` / `falling_edge()`
- Combinational processes with `process(all)` (VHDL-2008)
- Explicit sensitivity lists: `process(clk, rst)`
- Synchronous and asynchronous reset patterns
- Variable declarations within process scope

**Sequential Statements**
- `if` / `elsif` / `else` chains
- `case` / `when` with choices and `when others`
- `for` loops with discrete ranges
- `while` loops
- Signal assignments (`<=`)
- Variable assignments (`:=`)
- `return` in functions
- `next` and `exit` (loop control)
- `null` statement
- `assert` / `report` / `severity`

**Concurrent Statements**
- Concurrent signal assignments: `y <= a and b;`
- Conditional assignments: `y <= a when sel = '1' else b;`
- Selected assignments: `with sel select y <= ...`
- Component instantiation with port map and generic map
- Direct entity instantiation: `u : entity work.foo port map(...)`
- For-generate: `for i in 0 to N-1 generate ... end generate;`
- If-generate: `if CONDITION generate ... end generate;`
- Block statements

**Type Declarations**
- Enumeration types: `type state_t is (idle, running, done);`
- Record types: `type rec_t is record ... end record;`
- Constrained arrays: `type mem_t is array (0 to 255) of ...;`
- Unconstrained arrays: `type vec_t is array (natural range <>) of ...;`
- Subtype declarations: `subtype byte_t is unsigned(7 downto 0);`
- Integer range types: `type addr_t is range 0 to 1023;`

**Functions and Procedures**
- Function declarations and bodies (synthesizable subset)
- Procedure declarations and bodies
- Parameter modes: `in`, `out`, `inout`
- Function calls in expressions

**Expressions**
- Arithmetic: `+`, `-`, `*`, `/`, `mod`, `rem`, `**`
- Logical: `and`, `or`, `xor`, `nand`, `nor`, `xnor`, `not`
- Relational: `=`, `/=`, `<`, `>`, `<=`, `>=`
- Shift: `sll`, `srl`, `sla`, `sra`, `rol`, `ror`
- Concatenation: `&`
- Aggregate expressions: `(others => '0')`, `(0 => '1', others => '0')`
- Type conversions: `unsigned(x)`, `std_logic_vector(y)`, `to_integer(z)`
- Qualified expressions: `state_t'(idle)`
- Attribute access: `'range`, `'high`, `'low`, `'left`, `'right`, `'event`, `'length`
- Bit string literals: `X"FF"`, `O"77"`, `B"1010"`

**IEEE Standard Library**
- `ieee.std_logic_1164` — built-in type definitions
- `ieee.numeric_std` — `unsigned`, `signed`, arithmetic operations
- `std.standard` — implicit standard library

**Aliases**
- Signal aliases: `alias din : std_logic_vector(7 downto 0) is data_in;`

**Attributes**
- Signal attributes: `'event`, `'stable`
- Type/range attributes: `'range`, `'high`, `'low`, `'left`, `'right`, `'length`
- User-defined attributes via attribute declarations and specifications

**VHDL-2008 Features**
- `process(all)` — inferred sensitivity list
- Generic packages with type parameters
- Package instantiation with generic map

**VHDL-2019 Features**
- Interface declarations (signal bundles)
- Mode views (directional perspectives on interfaces)
- Generic type parameters on entities and packages

---

## Limitations and Roadmap

An honest list of what the VHDL frontend does not currently handle:

**Unsynthesizable by design (will never be supported):**
- `wait` statements
- `after` clauses and delay models
- `file` I/O and `access` types
- `shared variable`
- `transport` / `reject` delay mechanisms
- `force` / `release` (VHDL-2008)
- Physical types for timing
- `disconnect` / `guard`

These are simulation constructs. skalp's position is that simulation belongs in Rust testbenches, not in the HDL. This is a deliberate trade-off: engineers who want VHDL testbenches should use GHDL or a commercial simulator. Engineers who want modern testing infrastructure (async/await, property-based testing, parallel execution, coverage tracking) use skalp's testbench API.

**Not yet implemented (future work):**
- Multi-dimensional array slicing
- Full constraint resolution for unconstrained subtypes

**Known edge cases:**
- Integer range types without explicit bounds default to 32-bit width (matching the VHDL LRM minimum range for `integer`)
- Recursive function calls are unrolled up to 64 levels deep (inlining depth limit in the MIR pass); functions with more than 5 nested call sites are synthesized as separate modules rather than inlined

---

## How VHDL Designs Enter the Pipeline

To make the compilation flow concrete, here's what happens when skalp processes a VHDL file end-to-end. Consider a simple UART transmitter:

```vhdl
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity uart_tx is
    generic (
        CLK_FREQ  : positive := 100_000_000;
        BAUD_RATE : positive := 115200
    );
    port (
        clk      : in  std_logic;
        rst      : in  std_logic;
        tx_data  : in  unsigned(7 downto 0);
        tx_start : in  std_logic;
        tx_out   : out std_logic;
        tx_busy  : out std_logic
    );
end entity;
```

**Step 1: Lex.** The lexer produces a token stream: `LibraryKw`, `Ident("ieee")`, `Semi`, `UseKw`, `Ident("ieee")`, `Dot`, `Ident("std_logic_1164")`, `Dot`, `AllKw`, `Semi`, ... `EntityKw`, `Ident("uart_tx")`, `IsKw`, ... Each keyword is case-normalized; `ENTITY` and `entity` produce the same `EntityKw` token.

**Step 2: Parse.** The parser builds a lossless syntax tree. The `entity uart_tx` node contains a `GenericClause` with two `GenericDecl` children (`CLK_FREQ` and `BAUD_RATE`), a `PortClause` with six `PortDecl` children, and the architecture body with process statements, signal declarations, and concurrent assignments.

**Step 3: Lower to HIR.** The `HirEntity` produced has:
- `name: "uart_tx"`
- `generics: [HirGeneric { name: "clk_freq", param_type: Const(Nat(32)), default: Some(100_000_000) }, HirGeneric { name: "baud_rate", param_type: Const(Nat(32)), default: Some(115200) }]`
- `ports: [HirPort { name: "clk", direction: In, port_type: Logic(1) }, HirPort { name: "rst", direction: In, port_type: Logic(1) }, HirPort { name: "tx_data", direction: In, port_type: Nat(8) }, ...]`
- `assignments` and `clock_domains` populated from the architecture body

From this point, the UART transmitter is indistinguishable from one written in skalp. It flows through [MIR lowering](/blog/skalp-ir-pipeline/#hir--mir-lowering-intent-to-rtl) (monomorphizing the generics, generating sequential processes from the clocked logic), then to SIR for simulation or LIR for synthesis. The SystemVerilog codegen emits a clean `module uart_tx` with `always_ff` blocks. The GPU simulation backend dispatches the combinational and sequential kernels. The fault simulator injects stuck-at faults at every gate.

---

## Closing

The VHDL frontend is not about replacing VHDL with skalp. It's about removing the toolchain barrier that prevents VHDL designs from accessing modern analysis and verification capabilities.

Your VHDL IP stays in VHDL. Your architects can evaluate it with formal verification. Your safety engineers can run fault campaigns. Your verification team can write Rust async testbenches. Your physical design team gets clean SystemVerilog from the same compilation that feeds simulation.

The VHDL-2019 features — interfaces, views, type generics, generic packages — are a bonus. They let you write better VHDL today, with a tool that actually compiles it, while the rest of the industry catches up.

For the full story on how skalp's IR pipeline works, see [Four IRs Deep: How skalp Compiles Hardware](/blog/skalp-ir-pipeline/). For the design decisions behind the multi-frontend architecture, see [Why skalp Works the Way It Does](/blog/skalp-design-choices/#multi-frontend-ir-architecture). For GPU simulation details, see [GPU-Accelerated RTL Simulation](/blog/gpu-accelerated-simulation/). For skalp design patterns compared with SystemVerilog, see [Design Patterns in Real skalp Code](/blog/skalp-design-patterns/).

skalp is open source. Try it: [GitHub](https://github.com/girivs82/skalp) · [Tutorial](/tutorial/) · [Design Patterns](/blog/skalp-design-patterns/) · [Whitepaper](/blog/skalp-whitepaper/)
