---
title: "Chapter 4: Generics, Records, and Arrays"
date: 2026-03-04
summary: "Generic parameters, array types, register banks, double-flop synchronizers, and edge-detect interrupts — build a GPIO controller with skalp."
tags: ["skalp", "vhdl", "tutorial", "hardware"]
weight: 4
ShowToc: true
---

## What This Chapter Teaches

Real hardware is rarely a single counter or state machine — it is a collection of parameterized blocks stitched together. A GPIO controller must handle 8 pins, or 16, or 32, and it should not require rewriting the VHDL for each variant. VHDL solves this with **generics**: compile-time parameters that let you write one design and instantiate it with different widths, depths, or configuration options.

This chapter builds a GPIO controller that combines several common patterns:

- **Generic parameters** with default values for parameterized width
- **Register banks** with `with...select` read mux and `case` write decoder
- **Double-flop synchronizer** for safe asynchronous input sampling
- **Edge detection** with previous-value comparison
- **Interrupt generation** with OR accumulation and OR-reduce
- **Array types** for memories and register files

---

## The GPIO Controller

Create a file called `src/gpio_ctrl.vhd`:

```vhdl
library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity gpio_ctrl is
    generic (
        NUM_PINS : integer := 8
    );
    port (
        clk     : in  std_logic;
        rst     : in  std_logic;
        addr    : in  std_logic_vector(1 downto 0);
        wdata   : in  std_logic_vector(7 downto 0);
        rdata   : out std_logic_vector(7 downto 0);
        we      : in  std_logic;
        gpio_in : in  std_logic_vector(7 downto 0);
        gpio_out: out std_logic_vector(7 downto 0);
        gpio_dir: out std_logic_vector(7 downto 0);
        irq     : out std_logic
    );
end entity gpio_ctrl;

architecture rtl of gpio_ctrl is
    signal out_reg   : std_logic_vector(7 downto 0);
    signal dir_reg   : std_logic_vector(7 downto 0);
    signal irq_en    : std_logic_vector(7 downto 0);
    signal in_sync   : std_logic_vector(7 downto 0);
    signal in_prev   : std_logic_vector(7 downto 0);
    signal irq_pend  : std_logic_vector(7 downto 0);
    signal read_mux  : std_logic_vector(7 downto 0);
begin
    with addr select
        read_mux <= in_sync  when "00",
                    out_reg  when "01",
                    dir_reg  when "10",
                    irq_pend when others;

    rdata <= read_mux;

    reg_write: process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                out_reg <= (others => '0');
                dir_reg <= (others => '0');
                irq_en  <= (others => '0');
            elsif we = '1' then
                case addr is
                    when "01"   => out_reg <= wdata;
                    when "10"   => dir_reg <= wdata;
                    when "11"   => irq_en  <= wdata;
                    when others => null;
                end case;
            end if;
        end if;
    end process reg_write;

    sync: process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                in_sync <= (others => '0');
                in_prev <= (others => '0');
            else
                in_sync <= gpio_in;
                in_prev <= in_sync;
            end if;
        end if;
    end process sync;

    irq_gen: process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                irq_pend <= (others => '0');
            else
                irq_pend <= irq_pend or (irq_en and in_sync and (not in_prev));
            end if;
        end if;
    end process irq_gen;

    irq <= '1' when irq_pend /= "00000000" else '0';

    gpio_out <= out_reg;
    gpio_dir <= dir_reg;
end architecture rtl;
```

This is a substantial design. Let us walk through each section.

---

### Generics: Compile-Time Parameters

```vhdl
entity gpio_ctrl is
    generic (
        NUM_PINS : integer := 8
    );
    port ( ... );
end entity gpio_ctrl;
```

The `generic` clause comes before `port` in the entity declaration. `NUM_PINS : integer := 8` declares a generic parameter with a default value of 8. When you instantiate this entity, you can override the value:

```vhdl
u_gpio: entity work.gpio_ctrl
    generic map (NUM_PINS => 16)
    port map ( ... );
```

If you omit the `generic map`, it uses the default of 8.

Generics are constants resolved at elaboration time. You can use them in port widths (`std_logic_vector(NUM_PINS-1 downto 0)`), array bounds, loop ranges, and `generate` statements. In this design, `NUM_PINS` is declared but port widths are hardcoded to 8 for clarity. A fully parameterized version would use `NUM_PINS-1 downto 0` throughout.

---

### Register Bank: Read Multiplexer

```vhdl
with addr select
    read_mux <= in_sync  when "00",
                out_reg  when "01",
                dir_reg  when "10",
                irq_pend when others;

rdata <= read_mux;
```

This is a **concurrent selected signal assignment** — a mux table. The `with...select` construct maps each address value to a register output. The `when others` clause is required: VHDL demands exhaustive coverage of all selector values.

The register map is:

| Address | Register | Access |
|---------|----------|--------|
| `"00"` | `in_sync` (input data) | Read-only |
| `"01"` | `out_reg` (output data) | Read/Write |
| `"10"` | `dir_reg` (direction) | Read/Write |
| `"11"` | `irq_pend` (interrupt pending) | Read-only (via mux) |

The intermediate signal `read_mux` exists for readability. You could assign directly to `rdata`, but separating the mux output makes the design easier to extend later.

---

### Register Bank: Write Decoder

```vhdl
reg_write: process(clk)
begin
    if rising_edge(clk) then
        if rst = '1' then
            out_reg <= (others => '0');
            dir_reg <= (others => '0');
            irq_en  <= (others => '0');
        elsif we = '1' then
            case addr is
                when "01"   => out_reg <= wdata;
                when "10"   => dir_reg <= wdata;
                when "11"   => irq_en  <= wdata;
                when others => null;
            end case;
        end if;
    end if;
end process reg_write;
```

When `we` is asserted, the `case` statement routes `wdata` to the register selected by `addr`.

**The `null` statement** — `when others => null;` — explicitly says "do nothing." VHDL requires exhaustive `case` coverage, so `null` handles addresses with no write behavior. Omitting the branch entirely would be a compilation error.

The named process label `reg_write:` is optional but recommended — it produces clearer waveform hierarchies and better error messages.

---

### Double-Flop Synchronizer

```vhdl
sync: process(clk)
begin
    if rising_edge(clk) then
        if rst = '1' then
            in_sync <= (others => '0');
            in_prev <= (others => '0');
        else
            in_sync <= gpio_in;
            in_prev <= in_sync;
        end if;
    end if;
end process sync;
```

This is a **double-flop synchronizer**. External GPIO pins are asynchronous — sampling them directly can cause metastability. The double-flop chain adds two cycles of latency to safely cross from the asynchronous domain into the clock domain:

1. `in_sync <= gpio_in;` — first stage captures the input; may go metastable but has one clock period to settle.
2. `in_prev <= in_sync;` — second stage captures the settled value.

After two cycles, `in_sync` holds the synchronized input and `in_prev` holds its previous-cycle value — exactly what the edge detector needs.

**Signal semantics matter here.** All signal assignments in a process take effect at the end of the delta cycle. `in_prev <= in_sync` reads the *current* value of `in_sync`, not the new value being assigned on the same cycle. This is why two sequential assignments create a pipeline, not a short circuit.

---

### Edge Detection and Interrupt Generation

```vhdl
irq_gen: process(clk)
begin
    if rising_edge(clk) then
        if rst = '1' then
            irq_pend <= (others => '0');
        else
            irq_pend <= irq_pend or (irq_en and in_sync and (not in_prev));
        end if;
    end if;
end process irq_gen;
```

This single line packs three operations:

- **Edge detection**: `in_sync and (not in_prev)` is '1' for any bit that just transitioned low-to-high.
- **Interrupt masking**: `irq_en and (edges)` gates the detected edges — only enabled pins can trigger.
- **Pending accumulation**: `irq_pend or (masked_edges)` — once a bit is set, it stays set until software clears it.

The bitwise operators `and`, `or`, and `not` work element-by-element on `std_logic_vector` and are defined in `std_logic_1164`.

---

### OR-Reduce: Generating the Interrupt Output

```vhdl
irq <= '1' when irq_pend /= "00000000" else '0';
```

The `/=` operator is VHDL's not-equal comparison. If `irq_pend` is not all zeros, at least one interrupt is pending and `irq` goes high. This is an **OR-reduce** pattern — collapsing a vector to a single bit. VHDL-2008 supports `or irq_pend` as a unary reduction, but the not-equal-to-zero idiom is universally supported.

---

## Array Types

The GPIO controller uses `std_logic_vector` for its register bank, which works for fixed-width registers. But memories, FIFOs, and register files need an array of multi-bit words. VHDL handles this with **array type declarations**.

```vhdl
type mem_array is array(0 to 15) of std_logic_vector(7 downto 0);
signal memory : mem_array;
```

The `type` declaration creates a new array type: `array(<range>)` sets the index bounds and `of <element_type>` sets what each element holds. The element type can be any VHDL type — `std_logic`, `unsigned`, records, or even other arrays.

In a FIFO, array types combine with generics naturally:

```vhdl
type mem_array is array(0 to DEPTH-1) of std_logic_vector(WIDTH-1 downto 0);
signal memory : mem_array;
-- ...
memory(to_integer(wr_ptr)) <= din;   -- write
dout <= memory(to_integer(rd_ptr));   -- read
```

**Indexing** requires integers: `to_integer()` from `numeric_std` converts `unsigned` pointers. **Synthesis** maps arrays to block RAM, distributed RAM, or flip-flops depending on access patterns and depth. **Unconstrained arrays** use `range <>` to defer bounds — `std_logic_vector` itself is defined as `array(natural range <>) of std_logic`.

---

> **Coming from skalp?**
>
> | VHDL | skalp | Notes |
> |------|-------|-------|
> | `generic (NUM_PINS : integer := 8)` | `entity GpioCtrl[N: nat = 8]` | Compile-time parameters with defaults |
> | `generic map (NUM_PINS => 16)` | `GpioCtrl[16]` | Bracket syntax for generic arguments |
> | `type mem_array is array(0 to 15) of slv(...)` | `signal mem: nat[8][16]` | skalp arrays nest for multi-dimensions |
> | `memory(to_integer(idx))` | `mem[idx]` | No type conversion needed in skalp |
> | `null` (in case branch) | `_ => {}` (empty match arm) | Explicit "do nothing" |
> | `irq_pend /= "00000000"` | `irq_pend != 0` | skalp treats vectors as numbers |
>
> The biggest difference is type strictness: VHDL's `std_logic_vector`, `unsigned`, and `signed` are distinct types requiring explicit conversion. skalp's `bits[N]` and `nat[N]` handle both logical and arithmetic operations without casts.
>
> VHDL records (declared with `type T is record ... end record`) map directly to skalp structs. Both group related signals into a named bundle for cleaner port interfaces.

---

## Build and Test

Update `skalp.toml` to set the top entity:

```toml
[package]
name = "vhdl-tutorial"
version = "0.1.0"

[build]
lang = "vhdl"
top = "gpio_ctrl"
```

Build the design:

```bash
skalp build
```

### Testbench: Register Read/Write

Create `tests/gpio_ctrl_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_register_write_read() {
    let mut tb = Testbench::new("src/gpio_ctrl.vhd", "gpio_ctrl").await.unwrap();
    tb.reset(2).await;

    // Write 0xA5 to output register (addr = 01)
    tb.set("addr", 0b01u8);
    tb.set("wdata", 0xA5u8);
    tb.set("we", 1u8);
    tb.clock(1).await;

    // Stop writing
    tb.set("we", 0u8);

    // Read back output register (addr = 01)
    tb.set("addr", 0b01u8);
    tb.clock(1).await;
    tb.expect("rdata", 0xA5u32).await;

    // Check gpio_out reflects the written value
    tb.expect("gpio_out", 0xA5u32).await;

    // Write 0xFF to direction register (addr = 10)
    tb.set("addr", 0b10u8);
    tb.set("wdata", 0xFFu8);
    tb.set("we", 1u8);
    tb.clock(1).await;
    tb.set("we", 0u8);

    // Read back direction register
    tb.set("addr", 0b10u8);
    tb.clock(1).await;
    tb.expect("rdata", 0xFFu32).await;
    tb.expect("gpio_dir", 0xFFu32).await;
}
```

This test writes `0xA5` to the output register, reads it back via the read mux, verifies the `gpio_out` port reflects the value, then repeats the pattern for the direction register.

### Testbench: Edge-Detect Interrupt

```rust
#[tokio::test]
async fn test_edge_detect_interrupt() {
    let mut tb = Testbench::new("src/gpio_ctrl.vhd", "gpio_ctrl").await.unwrap();
    tb.reset(2).await;

    // No interrupt initially
    tb.expect("irq", 0u32).await;

    // Enable interrupt on bit 0
    tb.set("addr", 0b11u8);
    tb.set("wdata", 0x01u8);
    tb.set("we", 1u8);
    tb.clock(1).await;
    tb.set("we", 0u8);

    // gpio_in is all zeros — no edge yet
    tb.set("gpio_in", 0x00u8);
    tb.clock(2).await;  // two cycles for synchronizer latency
    tb.expect("irq", 0u32).await;

    // Rising edge on bit 0
    tb.set("gpio_in", 0x01u8);
    tb.clock(1).await;  // in_sync captures the new value
    tb.clock(1).await;  // in_prev gets the old value, edge detected
    tb.clock(1).await;  // irq_pend updated

    // Interrupt should now be asserted
    tb.expect("irq", 1u32).await;
}
```

This test verifies the interrupt path end-to-end. After reset, enable interrupts on bit 0, hold `gpio_in` low to establish a baseline, then drive bit 0 high. The synchronizer adds two cycles of latency, plus one cycle for `irq_pend` to update, so three clocks after the edge the interrupt asserts.

Run both tests:

```bash
cargo test
```

**Exercise:** Add a test that verifies interrupt masking. Enable interrupts on bits 0 and 2, drive a rising edge on bits 0, 1, and 2, and verify that only bits 0 and 2 appear in `irq_pend` (bit 1 should be masked). Read back `irq_pend` via address `"11"`.

---

## Quick Reference

| Concept | VHDL Syntax | Notes |
|---------|-------------|-------|
| Generic parameter | `generic (NAME : integer := 8)` | Declared before `port` in entity |
| Generic with default | `NAME : type := value` | Default used when `generic map` is omitted |
| Generic instantiation | `generic map (NAME => value)` | Override at instantiation time |
| Array type | `type T is array(0 to N) of elem_type` | Constrained array with fixed bounds |
| Unconstrained array | `type T is array(natural range <>) of elem` | Bounds supplied at use site |
| Array indexing | `memory(to_integer(idx))` | Index must be integer type |
| `with...select` | `with sel select sig <= a when "00", ...` | Concurrent selected assignment (mux) |
| `case` in process | `case sel is when "00" => ... end case;` | Sequential selected assignment |
| `null` statement | `when others => null;` | Explicit no-op in case branches |
| Bitwise AND | `a and b` | Element-by-element on vectors |
| Bitwise OR | `a or b` | Element-by-element on vectors |
| Bitwise NOT | `not a` | Element-by-element inversion |
| Not-equal | `a /= b` | Returns boolean; used for OR-reduce pattern |
| Conditional assign | `sig <= '1' when cond else '0'` | Concurrent conditional assignment |
| Record type | `type T is record ... end record;` | Groups signals into a named bundle |

---

## Next: Hierarchical Design

The GPIO controller is a self-contained block, but real systems wire many such blocks together. In Chapter 5, you will learn multi-entity designs, direct instantiation with `port map`, and how to connect components through a shared bus.

Continue to [Chapter 5: Hierarchical Design](../05-hierarchical-design/).
