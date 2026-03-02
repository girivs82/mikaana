---
title: "Why skalp Works the Way It Does: Design Choices and Their Justifications"
date: 2026-03-03
summary: "skalp makes deliberate departures from VHDL and SystemVerilog conventions. Some feel unfamiliar; all are motivated by real problems in production hardware design. Fifteen design choices explained — what skalp does, the common pushback, what traditional HDLs do, and why skalp's approach is right (with honest tradeoffs)."
tags: ["skalp", "hdl", "hardware", "language-design"]
ShowToc: true
---

skalp makes deliberate departures from VHDL and SystemVerilog conventions. If you've evaluated skalp — or argued with someone who has — you've probably heard some version of these objections: "Why not just use a process?" "A clock is just a signal." "Chisel already solved this in Scala." "Four IRs is over-engineered."

These are reasonable reactions. Every unfamiliar design choice costs adoption friction. The question is whether the benefit justifies the cost.

This post covers fifteen design choices in skalp, each following the same structure: what skalp does, what the pushback is, what traditional HDLs do instead, and why skalp's choice is right — including the honest tradeoffs where skalp pays a real cost.

For usage examples from real projects, see [Design Patterns in Real skalp Code](/blog/skalp-design-patterns/). For the full academic treatment, see the [whitepaper](/blog/skalp-whitepaper/). This post covers the *why*.

---

## Sequential/Combinational Separation with on() Blocks

**What skalp does.** All sequential logic lives inside `on(clk.rise)` blocks. Everything outside is combinational. There is no ambiguity about what is clocked and what isn't.

```
entity Counter {
    in clk: clock
    in rst: reset
    out count: nat[8]
}

impl Counter {
    signal counter: nat[8] = 0

    on(clk.rise) {
        if rst {
            counter = 0
        } else {
            counter = counter + 1
        }
    }

    count = counter  // combinational — outside on()
}
```

**The pushback.** "Feels lower-level — VHDL lets me describe behavior in one process." In VHDL, you can put combinational and sequential logic in the same `process`, and the synthesis tool infers what's registered:

```vhdl
process(clk)
begin
    if rising_edge(clk) then
        if rst = '1' then
            counter <= (others => '0');
        else
            counter <= counter + 1;
        end if;
    end if;
end process;
count <= counter;
```

SystemVerilog has the same pattern — `always_ff` for sequential and `always_comb` for combinational are *recommended* but not enforced. Nothing stops you from mixing sequential and combinational assignments in a single `always` block, or using the old-style `always @(*)` where the sensitivity list inference can silently go wrong.

**Why skalp's choice is right.** The VHDL single-process style works for simple counters. It breaks down when processes grow. A 200-line VHDL process with some signals assigned inside `if rising_edge(clk)` and others outside is a latch inference minefield — miss one branch and you've created unintended storage. SystemVerilog's `always_ff`/`always_comb` split is a step in the right direction, but it's advisory — the language still compiles if you violate the convention. Experienced engineers in both languages already separate sequential and combinational logic voluntarily. skalp makes it structural: if it's in `on()`, it's registered. If it's not, it's combinational. The compiler enforces what best practice recommends.

**Tradeoff.** Marginally more verbose for simple sequential logic. A two-line counter needs the `on()` wrapper. This buys structural clarity for the designs that actually matter — the 500-line state machines where a misplaced assignment creates a latch that passes simulation and fails in silicon.

**Future.** `intent` blocks will provide a behavioral layer that compiles down to explicit `on()` blocks, giving designers the concise syntax without the ambiguity.

---

## Clock and Reset as First-Class Types

**What skalp does.** `clock` and `reset(active_high)` / `reset(active_low)` are distinct types, not aliases for `bit`. The compiler tracks which clock domain every signal belongs to.

```
entity DualClockFIFO {
    in wr_clk: clock
    in rd_clk: clock
    in rst: reset(active_high)
    in wr_data: bit[8]
    out rd_data: bit[8]
}

impl DualClockFIFO {
    on(wr_clk.rise) {
        // signals here are in wr_clk domain
    }

    on(rd_clk.rise) {
        // signals here are in rd_clk domain
        // accessing a wr_clk-domain signal here is a compile error
        // unless it goes through a synchronizer
    }
}
```

**The pushback.** "Over-engineered — a clock is just a signal." In VHDL and SystemVerilog, clocks are `std_logic` or `logic` — the same type as any data signal:

```vhdl
entity dual_clock_fifo is
    port (
        wr_clk  : in  std_logic;  -- same type as data
        rd_clk  : in  std_logic;  -- hope you remember this is a different domain
        wr_data : in  std_logic_vector(7 downto 0)
    );
end entity;
```

**Why skalp's choice is right.** Clock domain crossings are the hardest class of bugs to find in hardware. They pass simulation (because simulators are cycle-accurate within a domain), pass synthesis (because synthesis tools don't check CDC by default), and fail intermittently in silicon. The industry response is post-synthesis lint tools like Spyglass CDC that produce hundreds of warnings, most of which are false positives.

skalp makes CDC violations compile errors. If you read a `wr_clk`-domain signal in an `rd_clk`-domain `on()` block without an explicit synchronizer, the code doesn't compile. Reset polarity is type-level too — `reset(active_high)` and `reset(active_low)` are different types, so you can't accidentally connect an active-low reset to an active-high port.

**Tradeoff.** Single-clock designs feel slightly over-specified. Declaring `clk: clock` instead of `clk: bit` for a design that only has one clock costs a few characters and buys nothing until you add a second domain. But single-clock designs don't stay single-clock forever, and when you add that second domain, the type system catches every crossing you missed.

---

## Two-State Logic Only (No X/Z)

**What skalp does.** `bit` is 0 or 1. There is no X (unknown), no Z (high-impedance), no U (uninitialized). Simulation uses randomized 0/1 initialization.

**The pushback.** "Loses information — X propagation catches bugs." Both VHDL and SystemVerilog use multi-valued logic — VHDL's `std_logic` has 9 states ('U', 'X', '0', '1', 'Z', 'W', 'L', 'H', '-'), SystemVerilog's `logic` has 4 (0, 1, X, Z). In both languages, uninitialized signals start as unknown and X propagation is supposed to flag bugs:

```systemverilog
logic [7:0] data;
// data is 8'bxxxxxxxx at time 0

always_ff @(posedge clk) begin
    if (condition)
        data <= input_a;
    // else: data retains X — propagates downstream
end

// Later: if (data == 8'hFF) — this is FALSE because X != anything
// But: if (data) — this is also X, which is... false? true? tool-dependent.
```

VHDL has the same behavior — `std_logic` signals default to 'U' (uninitialized), and the `std_logic_1164` resolution function propagates 'X' when multiple drivers conflict.

**Why skalp's choice is right.** X states are supposed to catch bugs, but they cause more bugs than they find. The fundamental problem: X propagation semantics don't match what the silicon does. Real flip-flops power up to 0 or 1, never X. When your simulation says a signal is X and your silicon says it's 0, the simulation is *less accurate* than a random 0/1 assignment.

Worse, X is pessimistic in boolean contexts. `X && 0` is `X` in simulation but `0` in silicon. This means X propagation can mask real bugs — a downstream comparison against X returns X (false), hiding a genuine logic error behind a wall of unknowns. Randomized 0/1 initialization, which skalp uses, actually finds more bugs because it exercises real execution paths instead of propagating "I don't know" through the entire design.

**What about `inout` and tristate?** skalp does support bidirectional ports (`inout`) and tristate drivers — these are I/O pad concerns, not value-domain concerns. A tristate buffer is a physical structure where the output is either driven or electrically disconnected. skalp models this with explicit `inout` ports and `driver` blocks that specify the enable condition, the driven value, and the bus:

```
entity MemoryInterface {
    inout data_bus: bit[16]
    in read_en: bit
    in write_en: bit
    in write_data: bit[16]
    out read_data: bit[16]
}

impl MemoryInterface {
    driver data_driver {
        enable: write_en,
        value: write_data,
        bus: data_bus
    }

    read_data = data_bus when read_en else 0
}
```

The key distinction: Z is not a *value* that propagates through logic — it's a *physical state* of an I/O pad. skalp treats it as a structural property of the driver, not a third state that `bit` can hold. The generated SystemVerilog emits proper `inout` ports and tristate assigns, but inside the design, every signal is 0 or 1.

**Tradeoff.** You can't use X as a "don't care" for synthesis optimization. In SystemVerilog, assigning `x` to an output tells the synthesis tool it can choose whatever value minimizes logic; VHDL's `'-'` (don't care) serves the same purpose. skalp uses explicit `_` wildcard patterns instead — you write `_ => ...` in a match expression to indicate you don't care about a case, but the meaning is syntactic (the compiler can optimize), not semantic (there is no "unknown" value at runtime).

---

## Single Assignment Model

**What skalp does.** `=` for assignments in `on()` blocks, continuous assignments outside. There is no blocking vs. non-blocking distinction. The `on()` block boundary defines what is sequential.

```
impl Counter {
    signal counter: nat[8] = 0

    on(clk.rise) {
        if rst {
            counter = 0      // sequential: inside on()
        } else {
            counter = counter + 1
        }
    }

    count = counter           // combinational: outside on()
}
```

**The pushback.** "SystemVerilog's blocking (`=`) and non-blocking (`<=`) assignments give me explicit control over scheduling." In SystemVerilog:

```systemverilog
always_ff @(posedge clk) begin
    a <= b;     // non-blocking: scheduled for end of time step
    b <= a;     // non-blocking: reads OLD a — correct swap
end

always_ff @(posedge clk) begin
    a = b;      // blocking: immediate — WRONG in always_ff
    b = a;      // blocking: reads NEW a — broken swap, silent bug
end
```

VHDL uses `<=` for all signal assignments (both clocked and combinational) and `:=` for variables. The distinction is different but equally confusing — variables update immediately while signals are deferred, and both can appear inside the same process:

```vhdl
process(clk)
    variable temp : std_logic_vector(7 downto 0);  -- immediate update
begin
    if rising_edge(clk) then
        temp := a;    -- variable assignment: immediate
        a <= b;       -- signal assignment: deferred to end of process
        b <= temp;    -- reads OLD a (captured in temp) — correct, but subtle
    end if;
end process;
```

**Why skalp's choice is right.** The blocking/non-blocking distinction is the single most common source of RTL bugs in SystemVerilog. Using `=` in `always_ff` is almost always wrong, but the language allows it. Using `<=` in `always_comb` creates race conditions, but the language allows that too. The semantics depend on context — the same operator means different things in different blocks.

VHDL's signal/variable split is safer in principle (signals are always deferred), but the `<=` operator is used for both clocked and combinational processes — you can't tell from the assignment whether you're in a clocked or combinational context without reading the enclosing process's sensitivity list.

skalp eliminates the distinction entirely. Inside `on()`, all assignments update registers at the clock edge. Outside `on()`, all assignments are continuous. The structural boundary — not the operator — determines the semantics. You cannot accidentally use the wrong assignment type because there is only one.

**Tradeoff.** One less degree of freedom. In SystemVerilog, you can use blocking assignments in `always_ff` for intermediate variables (a legitimate, if fragile, pattern). In skalp, intermediate values in `on()` blocks use `let` bindings, which are explicitly combinational-within-sequential. More verbose, but the intent is unambiguous.

---

## Entity/Impl Separation

**What skalp does.** `entity` declares the port interface. `impl` provides the logic. They are always separate blocks.

```
entity HierarchicalALU<const WIDTH: nat = 32> {
    in clk: clock
    in a: bit[WIDTH]
    in b: bit[WIDTH]
    in op: bit[3]
    out result: bit[WIDTH]
    out zero: bit
}

impl HierarchicalALU {
    signal result_comb: bit[WIDTH]

    result_comb = match op {
        0b000 => a + b,
        0b001 => a - b,
        0b010 => a & b,
        0b011 => a | b,
        _ => 0
    }

    zero = result_comb == 0
    result = result_comb
}
```

**The pushback.** "Why not a single module like SystemVerilog?" In SystemVerilog, ports and logic live in the same `module`:

```systemverilog
module hierarchical_alu #(parameter WIDTH = 32) (
    input  logic              clk,
    input  logic [WIDTH-1:0]  a, b,
    input  logic [2:0]        op,
    output logic [WIDTH-1:0]  result,
    output logic              zero
);
    // ports and logic in same block
    always_comb begin
        case (op)
            3'b000: result = a + b;
            // ...
        endcase
    end
endmodule
```

VHDL actually *does* separate interface from implementation — `entity` declares ports, `architecture` provides logic. skalp's entity/impl is directly inspired by this pattern. But VHDL allows multiple architectures per entity only in theory; in practice, synthesis tools bind entity to architecture by name convention, and most codebases use exactly one architecture per entity. skalp makes the separation mandatory and the multi-implementation pattern first-class.

**Why skalp's choice is right.** The interface is a contract. The implementation is a fulfillment of that contract. Separating them enables three things SystemVerilog can't do cleanly:

First, **multiple implementations of the same interface**. You can have `impl HierarchicalALU` for FPGA and a different `impl HierarchicalALU` for ASIC, both satisfying the same port contract. In SystemVerilog, you'd duplicate the entire module including the port list.

Second, **documentation and separate compilation**. The `entity` is a complete specification of what a module does (at the port level) without any implementation detail. Tools can process entities without reading impls.

Third, **the Rust trait pattern**. skalp's entity/impl mirrors Rust's struct/impl, making it immediately familiar to Rust-literate engineers — a growing population in the hardware verification and EDA space.

**Tradeoff.** Two blocks instead of one. For a simple inverter (`out y = !a`), the entity/impl separation adds three lines of boilerplate. The cost is constant; the benefit scales with design complexity.

---

## Match Expressions with Exhaustiveness Checking

**What skalp does.** `match` requires every possible case to be covered, or an explicit `_` default. Adding an enum variant without updating every `match` that uses it is a compile error.

```
pub enum ModuleState: bit[4] {
    Init = 0,
    WaitBms = 1,
    Precharge = 2,
    SoftStart = 3,
    Running = 4,
    Standby = 5,
    Derate = 6,
    Fault = 7,
    Shutdown = 8
}

// Every match must handle all 9 states:
match state_reg {
    ModuleState::Init => { /* ... */ },
    ModuleState::WaitBms => { /* ... */ },
    ModuleState::Precharge => { /* ... */ },
    ModuleState::SoftStart => { /* ... */ },
    ModuleState::Running => { /* ... */ },
    ModuleState::Standby => { /* ... */ },
    ModuleState::Derate => { /* ... */ },
    ModuleState::Fault => { /* ... */ },
    ModuleState::Shutdown => { /* ... */ }
    // Add ModuleState::Emergency to the enum? Compile error here.
}
```

**The pushback.** "VHDL `case` with `when others` is fine. SystemVerilog `default` catches everything." Both languages have the same pattern:

```systemverilog
// SystemVerilog
case (state)
    INIT:      master_enable <= 0;
    WAIT_BMS:  if (bms_connected) state <= PRECHARGE;
    // ... 7 more states ...
    default:   state <= INIT;  // "safe" catch-all
endcase
// Add EMERGENCY to the enum — default handles it. No warning. No error.
```

```vhdl
-- VHDL
case state is
    when INIT      => master_enable <= '0';
    when WAIT_BMS  => if bms_connected then state <= PRECHARGE; end if;
    -- ... 7 more states ...
    when others    => state <= INIT;  -- same trap, different syntax
end case;
-- Add EMERGENCY to the enum — when others handles it. No warning. No error.
```

**Why skalp's choice is right.** `default` / `when others` is a trap in both languages. It looks safe — "catch everything else and do something reasonable" — but it masks exactly the bugs you want to find. When you add `ModuleState::Emergency` to the enum, you *want* every state machine that dispatches on `ModuleState` to fail compilation until you've explicitly decided what `Emergency` means in that context. A silent fall-through to `INIT` is not a safety mechanism; it's a bug hiding behind the appearance of safety.

In VHDL, an incomplete `case` without `when others` infers a latch — unintended storage that passes synthesis and causes unpredictable behavior. In SystemVerilog, an incomplete `case` without `default` in `always_comb` triggers a lint warning at best. In skalp, it's a compile error. You must either handle every case or write `_ => ...` to explicitly acknowledge that you're grouping remaining cases.

**Tradeoff.** You must update every `match` when adding an enum variant. In a large design with 15 state machines dispatching on the same enum, adding a state means updating 15 match expressions. That's the feature, not the bug — each of those 15 locations is a place where the new state's behavior must be consciously decided.

---

## Strong Typing with No Implicit Conversions

**What skalp does.** `bit[8]`, `nat[8]`, and `int[8]` are distinct types. No implicit truncation, no implicit sign extension, no silent width mismatch.

```
entity DataProcessor {
    in raw_adc: nat[12]      // unsigned 12-bit
    in offset: int[16]       // signed 16-bit
    out result: int[16]
}

impl DataProcessor {
    // result = raw_adc + offset  // ERROR: nat[12] + int[16] — type mismatch
    result = raw_adc.as_int[16] + offset  // explicit: widen and convert
}
```

Compare with both traditional HDLs:

```systemverilog
// SystemVerilog — silently converts
logic [11:0] raw_adc;
logic signed [15:0] offset;
logic signed [15:0] result;

assign result = raw_adc + offset;  // silently zero-extends raw_adc,
                                    // result is unsigned because one operand is unsigned
                                    // ... or is it? depends on context. good luck.
```

```vhdl
-- VHDL — catches the type error, but the fix is verbose
signal raw_adc : unsigned(11 downto 0);
signal offset  : signed(15 downto 0);
signal result  : signed(15 downto 0);

-- result <= raw_adc + offset;  -- ERROR: can't mix unsigned and signed
result <= offset + signed(resize(raw_adc, 16));  -- explicit, but noisy
```

**The pushback.** "Too many casts — VHDL's type system is already strict enough." Width mismatches and sign-extension bugs are the second-most-common RTL bug category (after clock domain crossings). SystemVerilog silently truncates when you assign a wider signal to a narrower one. VHDL catches type mismatches between `signed` and `unsigned`, but the conversion functions (`resize`, `signed()`, `unsigned()`, `to_integer()`) are verbose and frequently combined wrong — `to_unsigned(to_integer(signed_val), 16)` is a common pattern that silently reinterprets negative values.

**Why skalp's choice is right.** Every explicit cast in skalp is a conscious decision about information loss. `raw_adc.as_int[16]` says: "I know this is an unsigned 12-bit value, and I want it as a signed 16-bit value, zero-extended." The programmer made the choice; the compiler verified the intent. In SystemVerilog, the compiler makes the choice silently, and it's often wrong.

Type aliases add domain clarity without runtime cost:

```
pub type MilliVolts = nat[16]
pub type MilliAmps = int[16]
pub type DeciCelsius = int[16]
```

When you see `signal v_bat: MilliVolts` in a codebase, you know what it represents without reading comments. When someone writes `temperature = voltage + current`, the aliases make the mistake visible during code review even though the compiler sees them as compatible integer types.

**Tradeoff.** More explicit casting. A design that mixes 8-bit, 16-bit, and 32-bit values will have more `.as_nat[N]` calls than equivalent SystemVerilog. Each one is a place where you thought about width and signedness. In practice, this catches real bugs during initial coding that would otherwise surface weeks later in gate-level simulation.

---

## Parametric Types and Const Generics

**What skalp does.** Entities can be generic over compile-time constants, and width computations can appear in type position.

```
entity FIFO<const WIDTH: nat = 8, const DEPTH: nat = 16> {
    in clk: clock
    in rst: reset(active_high)
    in wr_data: bit[WIDTH]
    out rd_data: bit[WIDTH]
    out count: nat[clog2(DEPTH + 1)]  // width computed from DEPTH
}

impl FIFO {
    signal memory: [bit[WIDTH]; DEPTH]
    signal wr_ptr: nat[clog2(DEPTH)]   // pointer width derived from depth
    signal rd_ptr: nat[clog2(DEPTH)]
    // ...
}
```

**The pushback.** "VHDL generics work fine." In VHDL:

```vhdl
entity fifo is
    generic (
        WIDTH : positive := 8;
        DEPTH : positive := 16;
        -- must manually compute pointer width:
        PTR_WIDTH : positive := 4;  -- hope someone keeps this in sync
        CNT_WIDTH : positive := 5   -- and this too
    );
    port (
        wr_data : in  std_logic_vector(WIDTH-1 downto 0);
        rd_data : out std_logic_vector(WIDTH-1 downto 0);
        count   : out std_logic_vector(CNT_WIDTH-1 downto 0)
    );
end entity;
```

SystemVerilog's `parameter` has the same problem — you can write `localparam PTR_WIDTH = $clog2(DEPTH)` inside the module, but you can't use it in the port declaration's type. You end up with `output logic [$clog2(DEPTH+1)-1:0] count` inline, which works but is noisy and error-prone for complex expressions.

**Why skalp's choice is right.** The VHDL FIFO has four generics where skalp has two. `PTR_WIDTH` and `CNT_WIDTH` are derived quantities — they must equal `clog2(DEPTH)` and `clog2(DEPTH+1)` respectively. But VHDL can't express that relationship in the generic declaration, so it becomes a documentation comment and a prayer that every instantiation gets it right. SystemVerilog can inline `$clog2()` but only in limited contexts, and the syntax quickly becomes unreadable for compound expressions.

skalp computes `clog2(DEPTH)` in type position. The pointer width is derived from the depth, and the compiler enforces the relationship. You can't instantiate `FIFO<8, 16>` with a 3-bit pointer — the type system makes it impossible.

Default values (`const WIDTH: nat = 8`) reduce boilerplate at instantiation. Generics over types (not just values) enable truly reusable IP — a FIFO generic over its data type, not just its width.

**Tradeoff.** The syntax is unfamiliar to VHDL designers. `<const DEPTH: nat>` looks like Rust generics because it is. Engineers coming from VHDL need to learn the notation. Once learned, it's strictly more expressive — everything VHDL generics can do, skalp const generics can do, plus width computation and type-level generics.

---

## Stream Types for Protocol Abstraction

**What skalp does.** `stream<T>` is a built-in type that carries implicit valid/ready handshaking. The compiler generates the handshake signals and enforces backpressure rules.

```
entity Transformer {
    in data_in: stream<bit[32]>     // valid + ready + 32-bit data
    out data_out: stream<bit[32]>
}
```

This is equivalent to manually writing:

```
entity Transformer {
    in data_in_valid: bit
    in data_in_data: bit[32]
    out data_in_ready: bit
    out data_out_valid: bit
    out data_out_data: bit[32]
    in data_out_ready: bit
}
```

**The pushback.** "I need full control over my bus protocol." In both VHDL and SystemVerilog, you'd write the handshake signals manually — the same 6 ports, the same direction ambiguity:

```systemverilog
// SystemVerilog
module transformer (
    input  logic        data_in_valid,
    input  logic [31:0] data_in_data,
    output logic        data_in_ready,
    output logic        data_out_valid,
    output logic [31:0] data_out_data,
    input  logic        data_out_ready
);
    // 6 ports for what is conceptually "data in, data out"
    // Did I get the directions right? Is ready an output or input?
    // For the producer port it's an output. For the consumer port it's an input.
    // Or is it the other way around? Check the AXI spec again.
endmodule
```

SystemVerilog has `interface`/`modport` to group related signals, but they're rarely used in practice and don't enforce protocol rules — a `modport` can't express "valid must not be deasserted while ready is low." VHDL-2019 added interfaces and views, which can express direction-dependent groupings (`view` lets the same record type have different port directions for master vs. slave). This is a real improvement over flat ports, but it's still structural grouping — the language doesn't know that the grouped signals form a valid/ready handshake, can't verify backpressure rules, and VHDL-2019 tool support remains limited in practice.

**Why skalp's choice is right.** Roughly 80% of module interfaces in a typical SoC are valid/ready handshakes. The `stream<T>` type encodes the protocol at the type level — you can't forget the ready signal, can't accidentally swap valid and ready directions, and the compiler can verify that backpressure is handled correctly. The six-port version in SystemVerilog is the same information with more surface area for errors.

For the 20% of interfaces that need custom protocols (multi-channel, credit-based flow control, custom handshakes), skalp provides `protocol` types that give full control over signal directions and timing relationships. Streams handle the common case; protocols handle everything else.

**Tradeoff.** Less flexible than raw signals for non-standard interfaces. If your protocol isn't valid/ready, `stream<T>` doesn't help and you fall back to explicit ports or `protocol` definitions. But for the majority of interfaces, the type-level protocol eliminates an entire class of integration bugs.

---

## Intent Annotations Instead of Vendor Pragmas

**What skalp does.** Design intent is expressed through first-class annotations that the compiler checks, propagates, and uses for optimization.

```
#[pipeline(stages=5, target_frequency="200MHz")]
entity ImageProcessor {
    in clk: clock
    in pixel_in: stream<bit[24]>
    out pixel_out: stream<bit[24]>
}

#[intent(style=parallel)]   // prefer carry-lookahead over ripple
signal sum: nat[32] = a + b

#[implements(SG001::TmrVoting)]
#[safety_mechanism(type=tmr)]
entity TmrVoter {
    in a: bit[8]
    in b: bit[8]
    in c: bit[8]
    out voted: bit[8]
    #[detection_signal]
    out fault_detected: bit
}
```

**The pushback.** "Pragmas work fine and every tool supports them." Both VHDL and SystemVerilog rely on pragmas and vendor attributes, with the same problems in both languages:

```systemverilog
// SystemVerilog (Xilinx)
(* dont_touch = "true" *)  // Xilinx-specific attribute syntax
module safety_voter (...);
    // synthesis translate_off
    // synopsys translate_off        -- wait, which tool am I targeting?
    // pragma protect begin_protected -- and this is encryption, not optimization

    (* max_fanout = 50 *)            // is this Xilinx or Synopsys syntax?
    logic [7:0] voted;
endmodule
```

```vhdl
-- VHDL (Xilinx)
attribute dont_touch : string;
attribute dont_touch of voted : signal is "true";  -- 2 lines for 1 constraint
attribute max_fanout : integer;
attribute max_fanout of voted : signal is 50;
-- Synopsys? Different attribute names. Intel? Different syntax entirely.
```

**Why skalp's choice is right.** Pragmas have three problems. First, they're unchecked — a typo in `(* dont_toch = "true" *)` is silently ignored. You've just lost your safety constraint and nothing told you. Second, they're vendor-specific — Xilinx, Intel, Cadence, and Synopsys each have their own syntax and supported attributes. Third, they're lost during compilation — a pragma in RTL doesn't survive synthesis into the gate-level netlist, so downstream tools (formal verification, safety analysis) can't see the original intent.

skalp's annotations are checked by the compiler (typos are errors), propagated through all four IRs (intent survives from source through synthesis to simulation), and composable (a safety annotation can reference a pipeline annotation). The `#[implements(SG001::TmrVoting)]` annotation creates a traceable link from the ISO 26262 safety goal to the gate-level implementation — automatically, not through a side-channel spreadsheet.

**Tradeoff.** skalp's intent vocabulary is still growing. Vendor pragmas have 30 years of ecosystem support and cover obscure optimization hints that skalp may not yet express. For constraints that skalp doesn't natively support, you can pass through raw attributes to the synthesis backend. But the core intents — pipeline, safety, power domain, implementation style — are first-class and checked.

---

## Rust-Inspired Module System

**What skalp does.** `mod`, `use`, and `pub` control visibility and imports, matching Rust's module system.

```
// lib/numeric/mod.sk
pub mod matrix;
pub mod vector;
pub mod cordic;

// src/main.sk
mod types;
mod async_fifo;

use types::{Vertex, TransformedVertex, Color};
use async_fifo::{AsyncFifo, clog2};

pub struct Color {
    pub r: bit[8],
    pub g: bit[8],
    pub b: bit[8],
    pub a: bit[8]
}
```

**The pushback.** "VHDL libraries and packages work fine." In VHDL:

```vhdl
library ieee;
use ieee.std_logic_1164.all;     -- imports EVERYTHING from std_logic_1164
use ieee.numeric_std.all;         -- imports EVERYTHING from numeric_std

library work;
use work.my_package.all;          -- imports EVERYTHING from my_package
-- want just one type? too bad. "use work.my_package.my_type" works
-- but nobody uses it because "all" is the convention
```

SystemVerilog's `import` is slightly better — `import pkg::SpecificType;` imports a single item — but there's no visibility control (everything in a `package` is public), and `import pkg::*` wildcard imports are the dominant convention.

**Why skalp's choice is right.** VHDL's `use ... all` is coarse-grained — it imports everything from a package into the current namespace. SystemVerilog's `import pkg::*` has the same problem. Both work for small projects but cause naming conflicts in large designs with multiple IP libraries. Neither language has visibility control — everything in a VHDL package or SystemVerilog package is public.

skalp's `use types::Vertex` imports exactly one type. `pub` controls what's visible outside a module — internal signals and helper types stay private. Hierarchical modules (`lib::numeric::matrix::Mat4x4`) scale to large designs without naming conflicts because each module is a namespace.

The module system also enables **separate compilation**. The compiler can process each module independently and check interfaces at module boundaries, without recompiling the entire design when one module changes.

**Tradeoff.** Different from VHDL convention. VHDL designers are used to `library`/`use`/`all` and will find `mod`/`use`/`pub` unfamiliar. The concepts map directly to Rust, Python, and Go module systems, so engineers with software experience adapt quickly. The learning cost is a few hours; the benefit is maintainable large-scale designs.

---

## Four-Level IR Pipeline (HIR → MIR → LIR → SIR)

**What skalp does.** Source code compiles through four distinct intermediate representations, each serving a different consumer.

```
skalp source (.sk)
       │ parse + resolve
       ▼
      HIR  ── intent, generics, safety attributes, clock domains
       │ lower intent → RTL
       ▼
      MIR  ── cycle-accurate RTL, hierarchy, processes
      / \
     /   \
    ▼     ▼
  LIR    SIR
   │      │
   │      └── flat, topologically sorted, GPU-optimized simulation
   └── word-level primitives, technology mapping, AIG optimization
```

**The pushback.** "Over-engineered — Chisel/FIRRTL uses one IR with lowering levels. Yosys uses RTLIL. Why four?"

**Why skalp's choice is right.** Each IR exists because its consumer needs a fundamentally different representation:

- **HIR** preserves source-level structure — generics, type aliases, pipeline annotations. This is what the code generator reads to emit SystemVerilog, what the LSP server uses for refactoring, and what documentation tools consume.
- **MIR** is cycle-accurate RTL — processes with sensitivity lists, registers with clock edges, module instances with port connections. This is what formal verification and equivalence checking consume.
- **LIR** is word-level gates — adders, multiplexers, flip-flops, AIG nodes. This is what synthesis and technology mapping consume.
- **SIR** is flat and topologically sorted — separate combinational and sequential node lists optimized for GPU-parallel simulation. This is what Metal shaders and compiled C++ consume.

A single IR with "levels" (FIRRTL's approach) means every consumer parses features it doesn't care about and misses features it needs. Intent annotations illustrate this: they exist in HIR (for documentation), propagate as metadata through MIR (for verification traceability), influence gate selection in LIR (for synthesis), and are irrelevant in SIR (simulation doesn't care about intent). A single IR would either drop intents too early or carry dead weight through simulation.

**Tradeoff.** Four lowering passes means four places for bugs. Each HIR→MIR, MIR→LIR, MIR→SIR, and LIR→SIR transformation is a potential source of miscompilation. But each pass is simpler than a monolithic transformation — MIR→SIR doesn't need to handle generics (resolved in HIR→MIR), and HIR→MIR doesn't need to handle gate-level decomposition (that's MIR→LIR). Simple passes are easier to test, easier to verify, and easier to debug when they fail.

For more detail, see [Four IRs Deep: How skalp Compiles Hardware](/blog/skalp-ir-pipeline/).

---

## A New Language vs. Embedded DSL

**What skalp does.** skalp is a standalone language with its own compiler, parser, type checker, LSP server, and error messages. It is not a library embedded in Rust, Scala, or Python.

**The pushback.** "Chisel embeds in Scala. Amaranth embeds in Python. SpinalHDL embeds in Scala. Why not embed in a real language and get its ecosystem for free?"

**Why skalp's choice is right.** Embedded DSLs inherit the host language's semantics, and host language semantics are wrong for hardware. Consider what you inherit:

**Error messages.** When a Chisel design has a type error, you get a Scala compiler error pointing at Scala source, with Scala type signatures, referencing Scala classes. The error says `found: chisel3.UInt, required: chisel3.Bool` — not "port `enable` expects `bit`, got `nat[8]`." The abstraction leaks at the point where you need it most: when something goes wrong.

```
// Chisel error (Scala stack trace):
// [error] /src/ALU.scala:42:15: type mismatch;
// [error]  found   : chisel3.core.UInt
// [error]  required: chisel3.core.Bool

// skalp error (hardware-specific):
// error[E0308]: type mismatch in port connection
//   --> src/alu.sk:42:15
//   |
// 42|     enable: data_valid,
//   |             ^^^^^^^^^^ expected `bit`, found `nat[8]`
//   |
//   = note: `nat[8]` cannot be implicitly converted to `bit`
```

**Type system.** Scala's type system wasn't designed for hardware. It can't express "this signal is in clock domain A" or "this width must equal clog2(N)" without encoding tricks that produce incomprehensible error messages when they fail. skalp's type system was designed for hardware from the ground up — clock domain tracking, width arithmetic in types, and CDC violations are first-class concepts, not library encodings.

**Tooling.** An LSP server for Chisel understands Scala — it can rename a Scala variable, find Scala references, show Scala types. It doesn't understand hardware — it can't show you which clock domain a signal belongs to, or highlight a CDC crossing, or generate a schematic from the design hierarchy. skalp's LSP understands the actual design, not the metaprogram that generates it.

**The ecosystem argument is weaker than it appears.** Hardware designers don't need Scala's collections library or Python's pandas. They need synthesis, simulation, formal verification, and waveform viewing — which skalp provides natively. What they lose is the host language's package manager and community. What they gain is a compiler that actually understands what they're building.

**Tradeoff.** Smaller ecosystem. No host-language libraries for testbench scripting (skalp uses Rust for testbenches, which has its own mature ecosystem). Learning a new syntax — though the syntax is small (smaller than SystemVerilog) and familiar to anyone who's written Rust. The cost is real; the payoff is a toolchain where every component — from parser to simulator to formal engine — understands hardware, not metaprogramming.

---

## Multi-Frontend IR Architecture

**What skalp does.** VHDL, and in the future SystemVerilog, compile to the same MIR and SIR that skalp source does. Backend tools (simulation, synthesis, equivalence checking, formal verification, safety analysis) are language-agnostic.

```
skalp source (.sk) ──→ HIR ──→ MIR ──→ LIR/SIR ──→ synthesis/simulation
                                 ▲
VHDL source (.vhd)  ─────────→ MIR ──→ LIR/SIR ──→ synthesis/simulation
                                 ▲
SystemVerilog (.sv)  ─────────→ MIR ──→ LIR/SIR ──→ synthesis/simulation
  (future)
```

**The pushback.** "Why support legacy languages if you built a better one?"

**Why skalp's choice is right.** Three reasons.

First, **migration path**. No team rewrites a million-line VHDL codebase to adopt a new language. With the VHDL frontend, engineers can use skalp's toolchain — its simulator, its formal engine, its safety analysis — on existing code today, and incrementally write new modules in skalp. Legacy VHDL and new skalp modules go through the same pipeline, interoperate at the MIR level, and share the same backend tools.

Second, **backend robustness**. Every frontend stress-tests the IR and the backend. The VHDL frontend already found two bugs in MIR→SIR synthesis (last-write-wins semantics in process lowering, and Case construct traversal in target analysis) that skalp's own patterns hadn't triggered. Different source languages exercise different corners of the IR, making the backend more robust for all languages.

Third, **tool investment leverage**. Building a GPU-accelerated simulator, an AIG-based synthesis engine, a formal verification framework, and an ISO 26262 safety analysis tool is expensive. Making those tools language-agnostic means the investment pays off for VHDL users, SystemVerilog users, and skalp users simultaneously. A synthesis bug fix benefits everyone.

**Tradeoff.** The VHDL frontend can't express skalp-specific features — intents, streams, protocols, compile-time CDC checking. Those require skalp source. VHDL goes through the pipeline as cycle-accurate RTL (MIR), not as intent-rich HIR. You get skalp's backend tools but not skalp's frontend safety guarantees.

---

## Building Synthesis and Backend Tools

**What skalp does.** skalp includes its own AIG-based logic optimization, technology mapping, and — for open FPGAs like Lattice iCE40 — a complete place-and-route pipeline that produces bitstreams directly. For ASIC and proprietary FPGA targets, skalp generates clean SystemVerilog and hands off to vendor tools (Vivado, Quartus, Design Compiler).

**The pushback.** "Why build synthesis tools? Synopsys, Cadence, and Xilinx have spent decades and billions of dollars optimizing synthesis. What can you hope to achieve?"

**Why skalp's choice is right.** skalp is not trying to replace Design Compiler for timing closure on a 3nm ASIC. That would be foolish. The strategy is more targeted: own the parts of the flow where intent preservation matters, and delegate commodity optimization to vendor tools that are genuinely better at it.

The primary output path for most designs is `skalp → SystemVerilog → vendor tools`. The generated SystemVerilog is clean, readable, and uses modern constructs (`always_ff`, `always_comb`, `logic`) that vendor tools optimize well. For this path, skalp is a frontend — a better way to write the RTL that Vivado or Design Compiler will synthesize.

But three things require skalp's own synthesis infrastructure:

First, **intent-aware optimization**. When LIR performs AIG rewriting and technology mapping, it carries safety annotations and intent metadata through every transformation. If a TMR voter is marked `#[safety_mechanism(type=tmr)]`, the optimizer won't merge its redundant logic — even if the three copies are functionally identical and a conventional optimizer would eagerly eliminate them. Vendor tools can't know this; they see raw SystemVerilog with no semantic annotations. You'd have to fight the optimizer with `dont_touch` pragmas and hope nothing slips through.

Second, **gate-level fault simulation**. ISO 26262 FMEDA requires injecting faults at every gate and measuring detection coverage. skalp's LIR→SIR path produces a flat gate-level netlist purpose-built for GPU-parallel fault injection — one Metal thread per fault, 11 million fault-cycle simulations per second. This isn't possible with vendor tools because they don't expose their gate-level netlist in a form suitable for GPU dispatch. You'd use a separate fault simulator (which costs six figures annually and runs on CPUs at a fraction of the speed).

Third, **open FPGA targets**. For FPGAs with documented bitstream formats — iCE40, ECP5, Gowin — skalp provides a complete flow from source to running hardware with zero vendor tool dependencies. The place-and-route uses simulated annealing placement and PathFinder routing, same algorithms as nextpnr but integrated into a single toolchain where intent annotations survive from source through bitstream. This matters for education, for open hardware, and for environments where installing 50GB of Vivado is not an option.

For everything else — Xilinx 7-series, Intel Stratix, TSMC standard cells — skalp generates SystemVerilog and gets out of the way. The vendor tools are better at timing-driven optimization for their own architectures, and pretending otherwise would be dishonest.

**Tradeoff.** skalp's synthesis engine is younger than ABC (let alone Synopsys). AIG rewriting and technology mapping are functional but not yet competitive with mature tools on raw QoR metrics. For the ASIC flow, this doesn't matter — you're using Design Compiler anyway.

For FPGAs, the economics of "suboptimal synthesis" are fundamentally different from ASIC. On an ASIC, every extra gate costs die area, which costs money per chip at volume. On an FPGA, you already bought the chip — the LUTs are there whether you use them or not. If your design occupies 50% of an iCE40 and skalp's synthesis is 25% less area-efficient than Yosys, you're at 62.5% utilization. The same hardware, the same cost, the same power envelope. You only care about synthesis efficiency when you're pushing against the resource ceiling, and most FPGA designs aren't — they're chosen with headroom precisely because FPGAs aren't area-optimized in the first place. The practical threshold is "does it fit and meet timing," not "is every LUT optimally packed."

The bet is that intent preservation and integrated safety analysis are worth more than the last percentage points of area optimization — and that the synthesis engine will improve over time while the architectural advantage of intent-aware optimization is permanent.

---

## Conclusion

skalp's design choices are not arbitrary departures from convention. Each addresses a real, documented problem in hardware design:

- **Sequential/combinational separation** eliminates latch inference
- **First-class clock and reset types** catch CDC violations at compile time
- **Two-state logic** removes simulation/synthesis mismatches
- **Single assignment model** eliminates blocking/non-blocking confusion
- **Entity/impl separation** enables multiple implementations and separate compilation
- **Exhaustive match** forces conscious handling of every case
- **Strong typing** catches width and signedness bugs at compile time
- **Const generics** compute derived widths in type position
- **Stream types** encode protocols at the type level
- **Intent annotations** survive compilation instead of being silently dropped
- **Rust-style modules** provide precise imports and visibility control
- **Four IRs** give each consumer the representation it needs
- **Standalone language** produces hardware-native error messages and tooling
- **Multi-frontend architecture** leverages backend investment across languages
- **Own synthesis infrastructure** preserves intent through optimization where vendor tools can't

Some of these add friction for simple designs. All of them pay dividends as complexity grows. The language is opinionated where opinions prevent bugs, and flexible where flexibility matters.

skalp is open source. Try it: [GitHub](https://github.com/girivs82/skalp) · [Tutorial](/tutorial/) · [Design Patterns](/blog/skalp-design-patterns/) · [Whitepaper](/blog/skalp-whitepaper/)
