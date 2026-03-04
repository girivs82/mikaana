---
title: "Chapter 2: Combinational Logic"
date: 2026-03-04
summary: "Multiplexers, process(all), case/when, concurrent assignments, when...else, and with...select — all the ways VHDL expresses combinational logic, compiled with skalp."
tags: ["skalp", "vhdl", "tutorial", "hardware"]
weight: 2
ShowToc: true
---

## What This Chapter Teaches

Combinational logic has no memory and no clock. The outputs depend only on the current inputs — change an input, and the output updates immediately. In VHDL, there are several ways to express the same combinational function, and each has its place. This chapter walks through all of them using a single design: a 4-to-1 multiplexer.

By the end of this chapter you will understand:

- How `process(all)` defines a combinational process (VHDL-2008)
- How `case/when/others` selects between alternatives inside a process
- How `when...else` expresses conditional signal assignment concurrently
- How `with...select` expresses selected signal assignment concurrently
- The difference between `std_logic_vector` and `unsigned` and when to use each
- That the absence of a clock edge means skalp infers purely combinational logic — no registers, no flip-flops

These are the fundamental building blocks for datapath logic: ALUs, decoders, encoders, priority arbiters, and bus multiplexers all follow these patterns.

---

## The Design: 4-to-1 Multiplexer

A 4-to-1 mux routes one of four 8-bit inputs to a single 8-bit output, selected by a 2-bit control signal. It is the simplest nontrivial combinational circuit and a good vehicle for comparing VHDL styles.

Create a file called `src/mux4.vhd`:

```vhdl
library ieee;
use ieee.std_logic_1164.all;

entity mux4 is
    port (
        a   : in  std_logic_vector(7 downto 0);
        b   : in  std_logic_vector(7 downto 0);
        c   : in  std_logic_vector(7 downto 0);
        d   : in  std_logic_vector(7 downto 0);
        sel : in  std_logic_vector(1 downto 0);
        y   : out std_logic_vector(7 downto 0)
    );
end entity mux4;

architecture rtl of mux4 is
begin

    process(all)
    begin
        case sel is
            when "00" =>
                y <= a;
            when "01" =>
                y <= b;
            when "10" =>
                y <= c;
            when others =>
                y <= d;
        end case;
    end process;

end architecture rtl;
```

### What Is Happening Here

**`library ieee; use ieee.std_logic_1164.all;`** imports the standard logic types. Every VHDL design that uses `std_logic` or `std_logic_vector` needs this. The `ieee.std_logic_1164` package defines the nine-valued logic system (`'0'`, `'1'`, `'Z'`, `'X'`, `'U'`, `'W'`, `'L'`, `'H'`, `'-'`) that models real hardware behavior.

**`entity mux4`** declares the interface. Four 8-bit data inputs (`a` through `d`), a 2-bit select input, and one 8-bit output. The `downto` direction means bit 7 is the MSB and bit 0 is the LSB — this is the standard convention for data buses.

**`architecture rtl of mux4`** contains the implementation. The name `rtl` is a convention meaning "register transfer level," though this particular design has no registers. You could name the architecture anything.

**`process(all)`** is a VHDL-2008 feature. The keyword `all` in the sensitivity list means "this process is sensitive to every signal it reads." When any input changes, the process re-evaluates. This is the correct and modern way to write combinational processes — it eliminates an entire class of simulation mismatches caused by incomplete sensitivity lists.

**`case sel is ... end case;`** is the VHDL selection construct. Each `when` branch matches a specific value of `sel`. The `when others` branch is the catch-all — it handles any value not explicitly listed. For a 2-bit `std_logic_vector`, the possible values include not just `"00"` through `"11"` but also meta-values like `"XX"` and `"UU"`. The `others` branch handles all of them.

**`y <= a;`** is a signal assignment. Inside a process, `<=` assigns to a signal. The value takes effect when the process suspends (at the `end process`), not immediately. For combinational processes this distinction rarely matters, but it is important to understand when you move to sequential logic.

### No Clock Means Combinational

Notice what is absent: there is no `rising_edge(clk)` and no clock port. skalp reads this design and sees a process with no clock edge — it infers purely combinational logic. The generated hardware is a multiplexer built from LUTs, with no flip-flops anywhere.

This is exactly how it should work. skalp does not require annotations or pragmas to distinguish combinational from sequential logic. The structure of your VHDL code tells it everything it needs to know.

---

## Alternative VHDL Styles

VHDL provides three ways to express the same combinational logic. The `process` + `case` style above is the most flexible, but for simple one-output functions, concurrent signal assignments are shorter and often clearer.

### Conditional Signal Assignment: `when...else`

```vhdl
architecture rtl_when_else of mux4 is
begin
    y <= a when sel = "00" else
         b when sel = "01" else
         c when sel = "10" else
         d;
end architecture rtl_when_else;
```

This is a **concurrent** statement — it sits directly in the architecture body, outside any process. It reads like a priority chain: check `sel = "00"` first, then `"01"`, then `"10"`, and fall through to `d` if none match. The result is identical hardware to the `case` version, but the priority encoding is explicit in the syntax.

Use `when...else` when you have a single output driven by a simple condition chain. It is the closest VHDL equivalent to a ternary expression.

### Selected Signal Assignment: `with...select`

```vhdl
architecture rtl_with_select of mux4 is
begin
    with sel select
        y <= a when "00",
             b when "01",
             c when "10",
             d when others;
end architecture rtl_with_select;
```

This is also a concurrent statement. It mirrors the `case` construct but lives outside a process. The `with sel select` names the selector once, and each `when` branch maps a value to an output. The `when others` clause is required if you do not enumerate every possible value.

Use `with...select` when the selection is a clean table lookup — one selector, one output, exhaustive coverage. It maps directly to a multiplexer in hardware.

### Comparing the Three Styles

| Style | Where it lives | Best for |
|-------|---------------|----------|
| `process` + `case` | Inside a process block | Multiple outputs, complex logic, nested conditions |
| `when...else` | Concurrent (architecture body) | Single output, priority-encoded conditions |
| `with...select` | Concurrent (architecture body) | Single output, clean table lookup by one selector |

All three produce the same hardware for a simple mux. The choice is readability. For this 4-to-1 mux, `with...select` is arguably the cleanest because the intent — "select one of four based on `sel`" — is immediately obvious. For more complex combinational logic with multiple outputs or nested conditions, a `process` block gives you the full power of sequential VHDL statements (`if`, `case`, `for` loops).

skalp compiles all three styles. You do not need to rewrite existing VHDL to match a preferred style.

### `process(all)` vs Explicit Sensitivity Lists

Before VHDL-2008, you had to list every signal that the process reads:

```vhdl
process(a, b, c, d, sel)
begin
    case sel is
        ...
    end case;
end process;
```

If you forgot a signal — say, you wrote `process(a, b, c, sel)` and omitted `d` — simulation would still work most of the time, but the process would not re-evaluate when `d` changes. This creates a mismatch between simulation and synthesis, because synthesis tools infer the correct sensitivity from the logic regardless of what you wrote.

`process(all)` eliminates this problem entirely. It is supported by VHDL-2008 and later, and skalp fully supports it. Use `process(all)` for all new combinational processes. There is no reason to write explicit sensitivity lists for combinational logic anymore.

---

## `std_logic_vector` vs `unsigned`

The mux above uses `std_logic_vector` for everything, including the selector `sel`. This works, but it means comparing against string literals like `"00"` and `"01"`. If you need arithmetic on a signal — comparing it to an integer, adding to it, using it as an array index — you should use `unsigned` from the `ieee.numeric_std` package instead.

For this mux, either type works. The general guideline:

| Type | When to use | Package |
|------|-------------|---------|
| `std_logic_vector` | Data buses, bit fields, signals where you care about individual bits | `ieee.std_logic_1164` |
| `unsigned` | Counters, addresses, arithmetic operands, array indices | `ieee.numeric_std` |
| `signed` | Signed arithmetic, two's complement values | `ieee.numeric_std` |

To convert between them:

```vhdl
use ieee.numeric_std.all;

-- std_logic_vector to unsigned:
signal sel_u : unsigned(1 downto 0);
sel_u <= unsigned(sel);

-- unsigned to std_logic_vector:
signal sel_v : std_logic_vector(1 downto 0);
sel_v <= std_logic_vector(sel_u);
```

skalp handles both types and the conversions between them. Use whichever type makes your intent clearest.

---

> **Coming from skalp?**
>
> In skalp's native language, combinational logic is a bare assignment at the `impl` level:
>
> ```
> // skalp combinational mux
> y = match sel {
>     0 => a,
>     1 => b,
>     2 => c,
>     _ => d,
> }
> ```
>
> No `process`, no sensitivity list, no `<=` vs `=` distinction. An assignment outside an `on(clk.rise)` block is combinational by definition.
>
> The VHDL equivalent requires more ceremony — a process with a sensitivity list, `case/when` syntax, signal assignment with `<=` — but the generated hardware is identical. skalp's VHDL frontend recognizes the combinational patterns (no clock edge in the process, concurrent signal assignments) and produces the same intermediate representation as native skalp code.
>
> | skalp | VHDL | Notes |
> |-------|------|-------|
> | `y = expr` (outside `on`) | `y <= expr;` in `process(all)` | Combinational assignment |
> | `match sel { ... }` | `case sel is ... end case;` | Selection construct |
> | `_ =>` (wildcard) | `when others =>` | Default branch |
> | No sensitivity list needed | `process(all)` or explicit list | skalp infers sensitivity automatically |
> | `nat[8]` | `unsigned(7 downto 0)` | Unsigned arithmetic type |
> | `bit[8]` | `std_logic_vector(7 downto 0)` | Uninterpreted bit vector |

---

## Build and Test

### Building the Mux

To compile the mux with skalp, update your project's `skalp.toml`:

```toml
[package]
name = "vhdl-tutorial"
version = "0.1.0"

[build]
top = "mux4"
```

Then build:

```bash
skalp build
```

Expected output:

```
   Compiling vhdl-tutorial v0.1.0
   Analyzing mux4
       Built mux4 -> build/mux4.sv
```

skalp parses the VHDL, verifies that all outputs are driven, checks for latches (there are none — every branch of the `case` assigns `y`), and generates synthesizable output.

### Common Errors

If you remove the `when others` branch, skalp will warn about an incomplete case statement. In combinational logic, every possible input combination must produce an output — otherwise the hardware infers a **latch**, which holds the previous value when no branch matches. Latches are almost always unintentional and a source of timing bugs. skalp flags them.

If you write `process(sel)` instead of `process(all)` and omit `a`, `b`, `c`, `d` from the sensitivity list, skalp will compile the design correctly (it infers the true sensitivity from the logic), but it will issue a warning about the incomplete sensitivity list. Fix it by using `process(all)`.

### Testing with a Rust Testbench

Create a test file at `tests/mux4_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_mux4_selects() {
    let mut tb = Testbench::new("src/mux4.vhd", "mux4").await.unwrap();
    tb.set("a", 0x11u32);
    tb.set("b", 0x22u32);
    tb.set("c", 0x33u32);
    tb.set("d", 0x44u32);

    tb.set("sel", 0u8);
    tb.clock(1).await;
    tb.expect("y", 0x11u32).await;

    tb.set("sel", 1u8);
    tb.clock(1).await;
    tb.expect("y", 0x22u32).await;

    tb.set("sel", 2u8);
    tb.clock(1).await;
    tb.expect("y", 0x33u32).await;

    tb.set("sel", 3u8);
    tb.clock(1).await;
    tb.expect("y", 0x44u32).await;
}
```

Run the test:

```bash
cargo test
```

The testbench drives all four data inputs with distinct values, then cycles through each selector value and checks that the correct input appears on the output. Even though this is combinational logic (no real clock edge matters), the `tb.clock(1).await` call advances the simulation by one time step so the testbench can observe the settled output.

**Exercise:** Add a test that changes the data inputs while holding `sel` constant. Verify that `y` tracks the selected input in real time — for example, set `sel = 1`, change `b` from `0x22` to `0xFF`, advance one cycle, and confirm `y` is `0xFF`.

---

## Quick Reference

| Concept | Syntax | Example |
|---------|--------|---------|
| Library import | `library ieee; use ieee.std_logic_1164.all;` | Required for `std_logic` types |
| Combinational process | `process(all) begin ... end process;` | VHDL-2008 auto sensitivity |
| Case selection | `case sel is when "00" => ... when others => ... end case;` | Inside a process |
| Conditional assignment | `y <= a when cond else b;` | Concurrent, priority encoded |
| Selected assignment | `with sel select y <= a when "00", b when others;` | Concurrent, table lookup |
| Signal assignment | `y <= expr;` | Inside process or concurrent |
| `std_logic_vector` | `std_logic_vector(7 downto 0)` | 8-bit uninterpreted vector |
| `unsigned` | `unsigned(7 downto 0)` | 8-bit unsigned (needs `numeric_std`) |
| Type conversion | `unsigned(slv)` / `std_logic_vector(u)` | Between vector and unsigned |
| `when others` | Required in `case` / `with...select` | Catch-all for unhandled values |

---

## Next: Clocked Processes and State Machines

The mux is purely combinational — no state, no memory, no clock. Real designs need to remember things across clock cycles. In Chapter 3, you will build clocked processes with `rising_edge(clk)`, enumerated types for state encoding, and a complete finite state machine. You will learn:

- How `if rising_edge(clk) then ... end if;` creates sequential logic
- How enumerated types (`type state_t is (IDLE, RUNNING, DONE)`) make FSMs readable
- How to combine combinational and sequential logic in the same architecture
- How skalp distinguishes registered outputs from combinational ones based on the process structure

Continue to [Chapter 3: Clocked Processes and State Machines](../03-processes-and-fsms/).
