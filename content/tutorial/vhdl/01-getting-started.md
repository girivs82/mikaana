---
title: "Chapter 1: Getting Started"
date: 2026-03-04
summary: "Your first VHDL design compiled with skalp — an 8-bit counter that introduces entity/architecture, ports, rising_edge, skalp build, and basic simulation."
tags: ["skalp", "vhdl", "tutorial", "hardware"]
weight: 1
ShowToc: true
---

## What This Chapter Teaches

skalp is not just a language — it is a hardware compiler with a VHDL frontend. If you already write VHDL, you can use skalp to compile, simulate, and test your designs without changing a single line of VHDL. No ModelSim license, no vendor toolchain, no testbench boilerplate in VHDL itself.

This chapter takes a standard 8-bit counter written in plain VHDL and walks it through skalp's workflow: project setup, build, simulation, and a Rust-based testbench.

By the end of this chapter you will understand:

- How to set up a skalp project for VHDL with `skalp.toml`
- How `skalp build` compiles `.vhd` files just like `.sk` files
- How `skalp sim` runs your VHDL design and produces VCD waveforms
- How to write a Rust testbench that drives inputs and checks outputs
- The VHDL constructs used in the counter: `entity`, `architecture`, `process`, `rising_edge`, `signal`, `std_logic`, `unsigned`, and `(others => '0')`

The VHDL code here is standard IEEE VHDL. No proprietary extensions, no skalp-specific syntax. Your existing designs work as-is.

---

## The Counter

Create a file called `src/counter.vhd`:

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
end entity counter;

architecture rtl of counter is
    signal count_reg : unsigned(7 downto 0);
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

end architecture rtl;
```

### What Is Happening Here

**`library ieee; use ieee.std_logic_1164.all; use ieee.numeric_std.all;`** — these library declarations import the standard types. `std_logic_1164` provides `std_logic` (the single-bit type with nine value states: '0', '1', 'Z', 'X', etc.). `numeric_std` provides `unsigned` and `signed` — numeric vector types that support arithmetic. Every VHDL design that does arithmetic needs both.

**`entity counter`** declares the hardware interface. It lists every port with a direction (`in` or `out`), a name, and a type. There is no logic here — only the contract that this hardware block exposes to the outside world. This is structurally identical to a skalp `entity` or a SystemVerilog `module` header.

**`architecture rtl of counter`** contains the behavior. An architecture is always associated with an entity by name. The `rtl` label is a convention (you could name it `behavioral` or anything else). Inside the architecture you write signal declarations, processes, and concurrent assignments.

**`signal count_reg : unsigned(7 downto 0);`** declares an internal signal. In VHDL, all internal state that persists across clock edges is declared as a signal in the architecture's declarative region (between `is` and `begin`). The type `unsigned(7 downto 0)` is an 8-bit unsigned vector — bits numbered 7 down to 0, giving a range of 0 to 255.

**`process(clk)`** defines a block of sequential statements that executes whenever `clk` changes. The sensitivity list `(clk)` tells the simulator to evaluate this process on any event on `clk`. Inside, `rising_edge(clk)` filters for the positive edge only. This is the standard VHDL pattern for registered logic — it is equivalent to `always_ff @(posedge clk)` in SystemVerilog or `on(clk.rise)` in skalp.

**`if rst = '1' then`** — VHDL uses `=` for comparison (not `==`). The reset check comes first inside the rising-edge block, making this a synchronous reset. When `rst` is high, the counter clears.

**`count_reg <= (others => '0');`** — the `<=` operator is signal assignment in VHDL (not to be confused with less-than-or-equal). The aggregate `(others => '0')` fills every bit of `count_reg` with '0'. This is VHDL's way of writing a zero value for any width — it works regardless of the signal's length.

**`elsif en = '1' then`** — when not in reset, if the enable input is high, the counter increments. The `+` operator works on `unsigned` because `numeric_std` defines arithmetic for it.

**`count <= count_reg;`** — a concurrent signal assignment outside the process. This continuously drives the output port `count` from the internal register. It is equivalent to `assign count = count_reg;` in SystemVerilog or `count = count_reg` at the impl level in skalp.

### Types You Have Seen

| Type | Package | Meaning |
|------|---------|---------|
| `std_logic` | `std_logic_1164` | Single-bit signal with nine states ('0', '1', 'Z', 'X', etc.) |
| `unsigned(N downto 0)` | `numeric_std` | (N+1)-bit unsigned integer vector, supports arithmetic |
| `'0'`, `'1'` | — | Character literals for single-bit values |
| `(others => '0')` | — | Aggregate that fills all bits with a given value |

`std_logic` is the universal single-bit type in VHDL. Unlike a plain `bit` type (which only has '0' and '1'), `std_logic` models real hardware with 'Z' for tristate, 'X' for unknown, and others. Virtually all VHDL code uses `std_logic` for ports.

`unsigned` from `numeric_std` is the preferred type for counters, addresses, and arithmetic. It behaves like a number — you can add, subtract, and compare. The older `std_logic_vector` is a bag of bits with no arithmetic interpretation; use `unsigned` when you need math.

> **Coming from skalp?**
>
> If you have worked through the skalp language tutorial, the mapping is direct:
>
> | skalp | VHDL | Notes |
> |-------|------|-------|
> | `entity Counter` | `entity counter is ... end entity;` | Same concept, more punctuation |
> | `impl Counter` | `architecture rtl of counter is ... end architecture;` | Architecture is always named and linked to an entity |
> | `on(clk.rise)` | `process(clk) begin if rising_edge(clk) then ...` | Same semantics, VHDL wraps it in a process |
> | `signal count_reg: nat[8]` | `signal count_reg : unsigned(7 downto 0);` | skalp uses bit width, VHDL uses index range |
> | `count = count_reg` | `count <= count_reg;` | skalp uses `=`, VHDL uses `<=` for signal assignment |
> | `if rst { ... }` | `if rst = '1' then ... end if;` | VHDL requires explicit comparison and `end if` |
> | No wire/reg distinction | No wire/reg distinction | Both languages infer registered vs. combinational from context |
>
> The biggest difference: in skalp, the compiler infers that a signal is a register because it is assigned inside `on(clk.rise)`. In VHDL, the same inference happens — a signal assigned inside a clocked process becomes a flip-flop. Neither language requires you to declare "this is a register" explicitly.

---

## Project Setup

skalp uses `skalp.toml` at the project root for configuration. For a VHDL project, you need to set `lang = "vhdl"` in the `[build]` section so the compiler knows to use the VHDL frontend instead of the skalp language parser.

Your project structure should look like this:

```
vhdl-tutorial/
  skalp.toml
  src/
    counter.vhd
```

If you created the project with `skalp new vhdl-tutorial` during the [tutorial introduction](../), edit the generated `skalp.toml` to match:

```toml
[package]
name = "vhdl-tutorial"
version = "0.1.0"

[build]
lang = "vhdl"
top = "counter"
```

Three things to note:

- **`lang = "vhdl"`** tells skalp to compile `.vhd` files in `src/`. Without this, skalp looks for `.sk` files.
- **`top = "counter"`** sets the top-level entity. This is the VHDL entity name, not the filename.
- **VHDL source files go in `src/`** with the `.vhd` extension, the same directory convention as skalp language files.

---

## Build and Simulate

### Building

Run the build command from the project root:

```bash
skalp build
```

If everything is correct, you will see output like:

```
   Compiling vhdl-tutorial v0.1.0
   Analyzing counter
       Built counter -> build/counter.vhd
```

The compiler parses `counter.vhd`, resolves the library references, type-checks the design, and produces an analyzed output in `build/`. The build step validates that your VHDL is correct — any syntax errors, type mismatches, or undeclared signals are reported here with source-level diagnostics pointing to the exact line and column.

### Simulating

skalp includes a built-in simulator. Run the counter for 300 cycles:

```bash
skalp sim --entity counter --cycles 300
```

This runs 300 clock cycles with default stimulus: reset is asserted for the first 10 cycles, then released. The `en` input defaults to high. You should see the counter increment from 0 upward.

To capture waveforms for viewing in GTKWave or any VCD-compatible viewer:

```bash
skalp sim --entity counter --cycles 300 --vcd build/counter.vcd
```

Open `build/counter.vcd` in your waveform viewer. You will see:

1. **Reset phase** (cycles 0-10): `count` stays at 0, `rst` is high
2. **Counting phase** (cycles 11-265): `count` ramps from 0 to 255
3. **Wrap-around** (cycle 266): `count` returns to 0 and continues counting

### Common Errors

If you see `error: port 'count' is never driven`, the concurrent assignment `count <= count_reg;` is missing or misplaced. It must appear inside the `begin...end` of the architecture, outside the process.

If you see `error: unknown identifier 'unsigned'`, you are missing the `use ieee.numeric_std.all;` library declaration. The `unsigned` type is not built into VHDL — it comes from the `numeric_std` package.

If you see `error: type mismatch in assignment`, check that your signal types match. VHDL is strictly typed — you cannot assign a `std_logic_vector` to an `unsigned` without an explicit type conversion.

---

## Testing Your Design

Writing testbenches in VHDL is verbose — you need a separate entity with no ports, a component instantiation, clock generation processes, and manual signal driving. skalp replaces all of that with Rust-based testbenches that are concise, async, and use Rust's test runner directly.

Here is the testbench for the counter. Create `tests/counter_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_counter_counts() {
    let mut tb = Testbench::new("src/counter.vhd", "counter").await.unwrap();
    tb.reset(2).await;
    tb.expect("count", 0u32).await;
    tb.set("en", 1u8);
    for i in 1..=10u32 {
        tb.clock(1).await;
        tb.expect("count", i).await;
    }
}
```

The pattern is simple:

1. **Create the testbench** with `Testbench::new()`. The first argument is the path to the VHDL source file, the second is the entity name. The function compiles and loads the design.
2. **Reset** with `tb.reset(2).await` — this asserts reset for 2 clock cycles, then releases it.
3. **Check outputs** with `tb.expect("count", 0u32).await` — this reads the current value of the `count` port and asserts it equals the expected value.
4. **Drive inputs** with `tb.set("en", 1u8)` — this sets the `en` input to 1. The value takes effect on the next clock edge.
5. **Advance time** with `tb.clock(1).await` — this runs one clock cycle.
6. **Loop and verify** — the `for` loop counts from 1 to 10, advancing one cycle at a time and checking that `count` matches.

Run it with:

```bash
cargo test
```

You should see:

```
running 1 test
test test_counter_counts ... ok

test result: ok. 1 passed; 0 finished in 0.12s
```

**Exercise:** Write a `test_counter_holds_when_disabled` test that enables counting to 5, then sets `en` to 0 for 10 cycles, and verifies the count is still 5 afterward. Then re-enable and verify it resumes from 5.

---

## Quick Reference

| Concept | VHDL Syntax | skalp.toml |
|---------|-------------|------------|
| Entity declaration | `entity name is port (...); end entity;` | `top = "name"` sets the top entity |
| Architecture | `architecture rtl of name is ... begin ... end;` | — |
| Input port | `name : in std_logic` | — |
| Output port | `name : out unsigned(7 downto 0)` | — |
| Internal signal | `signal name : type;` (in declarative region) | — |
| Clocked process | `process(clk) begin if rising_edge(clk) then ...` | — |
| Signal assignment | `name <= expr;` | — |
| Reset pattern | `if rst = '1' then ... elsif ...` | — |
| Zero aggregate | `(others => '0')` | — |
| Library import | `library ieee; use ieee.std_logic_1164.all;` | — |
| VHDL project | — | `lang = "vhdl"` in `[build]` section |
| Build | — | `skalp build` |
| Simulate | — | `skalp sim --entity name --cycles N` |
| Waveform dump | — | `skalp sim --vcd build/out.vcd` |
| Source directory | `.vhd` files in `src/` | — |
| Run tests | — | `cargo test` |

---

## Next: Combinational Logic

The counter is a purely sequential design — everything happens inside a single clocked process. Real hardware also needs combinational logic: multiplexers, decoders, priority encoders, and lookup tables that produce outputs immediately from inputs without waiting for a clock edge.

In Chapter 2, you will build a 4-to-1 multiplexer and learn:

- `process(all)` for combinational processes (VHDL-2008)
- `case` / `when` for multi-way selection
- Concurrent signal assignments with `when...else` and `with...select`
- How skalp handles sensitivity list inference
- The difference between combinational and sequential processes in VHDL

Continue to [Chapter 2: Combinational Logic](../02-combinational-logic/).
