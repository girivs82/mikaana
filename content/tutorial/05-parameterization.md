---
title: "Chapter 5: Const Generics and Parameterization"
date: 2025-07-15
summary: "Make your designs fully configurable with generic defaults, explicit instantiation, compile-time width computation, const expressions, and test vs. production parameterization -- all applied to the running UART project."
tags: ["skalp", "tutorial", "hdl", "hardware"]
weight: 5
ShowToc: true
---

## What This Chapter Teaches

In Chapter 4 you built a parameterized FIFO with generic `WIDTH` and `DEPTH` parameters and wired TX/RX FIFOs into the UART. That was your first taste of generics. This chapter goes much deeper into the parameterization system and shows how to make every aspect of a design configurable from the outside.

By the end of this chapter you will understand:

- How `const` parameters in an entity declaration create generic hardware
- How generic defaults let you omit parameters when instantiating
- How explicit instantiation like `Entity<16, 32>` overrides those defaults
- How `const` declarations inside `impl` compute derived constants at compile time
- How `clog2()` calculates bit widths from depth values
- How const expressions compose — adding, dividing, and referencing other consts
- How to use parameterization for testability: same logic, different timing constants for simulation vs. production
- That skalp evaluates all const expressions at compile time with full type checking

The running project becomes a fully parameterized UART where clock frequency, baud rate, data width, and FIFO depth are all configurable from the instantiation site. You will also see how to create a simulation configuration with fast timing that runs in a fraction of the cycles.

---

## Standalone Example: Parameterized Adder with Carry

Let us start with something simple. A parameterized adder takes two N-bit inputs and produces an N-bit sum plus a carry-out bit. The width is a generic parameter with a default of 8.

Create a file called `src/adder.sk`:

```
// A parameterized adder with carry-in and carry-out.
//
// The WIDTH parameter controls how wide the operands are.
// The result is computed using a (WIDTH + 1)-bit addition
// so that carry-out falls naturally into the extra bit.

entity Adder<const WIDTH: nat = 8> {
    in clk: clock,
    in rst: reset,
    in a: bit[WIDTH],
    in b: bit[WIDTH],
    in carry_in: bit,
    out sum: bit[WIDTH],
    out carry_out: bit
}

impl Adder {
    // Extend a and b by one bit, add them with carry_in,
    // then split the result into sum and carry.
    //
    // The "+:" operator is a widening add — it produces a
    // result one bit wider than the widest operand, so no
    // overflow occurs and the carry is captured.
    let result: bit[WIDTH + 1] = a +: b + carry_in

    // The lower WIDTH bits are the sum.
    sum = result[WIDTH - 1 : 0]

    // The top bit is the carry-out.
    carry_out = result[WIDTH]
}
```

### How Generics Work

The `<const WIDTH: nat = 8>` syntax in the entity declaration creates a generic parameter. Let us break this apart:

- `const` marks it as a compile-time constant. It is not a port. It does not exist as a signal in the generated hardware. It is resolved entirely by the compiler before synthesis.
- `WIDTH` is the parameter name. Inside the entity and impl, you use it as a value anywhere a constant is expected: port widths, array sizes, arithmetic expressions.
- `nat` is the parameter type. Generic parameters must be `nat` (unsigned integer). You cannot parameterize over types, only over values.
- `= 8` is the default. If the instantiation site does not specify WIDTH, it gets 8. Defaults are optional — you can omit them to force every instantiation to provide a value.

### Instantiation with Different Widths

Here is how you instantiate the adder at three different widths:

```
// Uses the default WIDTH = 8.
let adder8 = Adder {
    clk: clk,
    rst: rst,
    a: data_a[7:0],
    b: data_b[7:0],
    carry_in: 0
}

// Explicit 16-bit instantiation. Overrides the default.
let adder16 = Adder<16> {
    clk: clk,
    rst: rst,
    a: data_a[15:0],
    b: data_b[15:0],
    carry_in: adder8.carry_out
}

// Explicit 32-bit instantiation.
let adder32 = Adder<32> {
    clk: clk,
    rst: rst,
    a: wide_a,
    b: wide_b,
    carry_in: 0
}
```

The first instantiation omits the angle brackets entirely. Because `Adder` has a default for `WIDTH`, this is legal and produces an 8-bit adder. The second and third use `Adder<16>` and `Adder<32>` to override the default.

Each instantiation produces a separate piece of hardware with its own width. The compiler monomorphizes each variant — there is no runtime polymorphism. `Adder<8>` and `Adder<32>` are two different modules in the generated SystemVerilog.

### Const Declarations

You can also declare named constants at the top level or inside an `impl` to build up parameterized values:

```
// Top-level constants. These are visible to the entire file.
const DATA_WIDTH: nat = 32
const ADDR_WIDTH: nat = clog2(1024)    // = 10
const TOTAL_BITS: nat = DATA_WIDTH + ADDR_WIDTH  // = 42

// Constants can reference other constants.
const MAX_ADDR: nat = (1 << ADDR_WIDTH) - 1  // = 1023
```

Const declarations follow the form `const NAME: type = expr`. The expression is evaluated at compile time. You can use arithmetic (`+`, `-`, `*`, `/`), bit shifts (`<<`, `>>`), the `clog2()` function, and references to other constants.

`clog2(N)` computes the ceiling of log-base-2 of N. This tells you how many bits you need to represent N distinct values. `clog2(1024)` is 10. `clog2(1000)` is also 10. `clog2(256)` is 8. You will use this constantly for address widths, counter widths, and state encoding.

> **Coming from SystemVerilog?**
>
> Generics and constants map to familiar concepts, but with key differences:
>
> | SystemVerilog | skalp | Notes |
> |---------------|-------|-------|
> | `parameter WIDTH = 8` | `const WIDTH: nat = 8` | Parameters are always typed in skalp |
> | `localparam ADDR_W = $clog2(DEPTH)` | `const ADDR_W: nat = clog2(DEPTH)` | No `localparam` keyword — just `const` |
> | `logic [ADDR_W-1:0] addr` | `signal addr: bit[ADDR_W]` | No `-1` — width is the bit count |
> | `#(parameter WIDTH = 8)` | `<const WIDTH: nat = 8>` | Angle brackets, not `#(...)` |
> | `module_name #(.WIDTH(16)) inst (.a(a))` | `let inst = Entity<16> { a: a }` | Positional generics, named ports |
> | No parameterized functions | Generic functions supported | skalp generics work on functions too |
>
> The biggest wins: no `-1` in bit widths (you write `bit[ADDR_W]` instead of `logic [ADDR_W-1:0]`), no `localparam` boilerplate for derived constants, and stronger type checking on parameter values. If you pass a negative value where `nat` is expected, the compiler rejects it at compile time.

---

## Running Project: Fully Parameterized UART

The UART from Chapters 2-4 has several magic numbers baked in: 50 MHz clock frequency, 115200 baud rate, 8 data bits, a FIFO depth of 16, and the derived constant `CYCLES_PER_BIT = 434`. Let us replace all of them with generic parameters and derived constants so the same design works at any clock frequency, baud rate, or data width.

### The Top-Level Entity

Replace your `src/uart_top.sk` with a fully parameterized version:

```
// Fully parameterized UART with TX and RX, each with a FIFO.
//
// All timing constants are derived from CLK_FREQ_HZ and BAUD_RATE
// at compile time. DATA_BITS controls the word size (typically 8).
// FIFO_DEPTH sets the buffer depth for both TX and RX FIFOs.

entity UartTop<
    const CLK_FREQ_HZ: nat = 50_000_000,
    const BAUD_RATE: nat = 115200,
    const DATA_BITS: nat = 8,
    const FIFO_DEPTH: nat = 16
> {
    in clk: clock,
    in rst: reset,

    // TX interface: write side
    in tx_data: bit[DATA_BITS],
    in tx_valid: bit,
    out tx_ready: bit,

    // RX interface: read side
    out rx_data: bit[DATA_BITS],
    out rx_valid: bit,
    in rx_read: bit,

    // Serial lines
    out tx_serial: bit,
    in rx_serial: bit,

    // Status
    out tx_fifo_full: bit,
    out rx_fifo_empty: bit
}
```

Notice that every port width and buffer size is expressed in terms of the generic parameters. `tx_data` is `bit[DATA_BITS]` wide, not a hard-coded `bit[8]`. The FIFO depth is a parameter, not a constant baked into the design.

### Derived Constants Inside impl

The `impl` block is where the real power of const expressions shows up. All timing values are computed from the top-level generics:

```
impl UartTop {
    // Derived timing constants — computed at compile time.
    // These never appear as signals in hardware. They are resolved
    // by the compiler and inlined as literal values in the output.
    const CYCLES_PER_BIT: nat = CLK_FREQ_HZ / BAUD_RATE
    const HALF_BIT: nat = CYCLES_PER_BIT / 2
    const COUNTER_WIDTH: nat = clog2(CYCLES_PER_BIT)
    const BIT_COUNT_WIDTH: nat = clog2(DATA_BITS + 2)

    // FIFO address width derived from depth.
    const FIFO_ADDR_WIDTH: nat = clog2(FIFO_DEPTH)

    // Instantiate the TX and RX FIFOs with the parameterized depth.
    let tx_fifo = FIFO<DATA_BITS, FIFO_DEPTH> {
        clk: clk,
        rst: rst,
        write_en: tx_valid & tx_ready,
        write_data: tx_data,
        read_en: tx_fifo_read,
        read_data: tx_fifo_data,
        full: tx_fifo_full,
        empty: tx_fifo_empty_internal
    }

    let rx_fifo = FIFO<DATA_BITS, FIFO_DEPTH> {
        clk: clk,
        rst: rst,
        write_en: rx_byte_valid,
        write_data: rx_byte_data,
        read_en: rx_read,
        read_data: rx_data,
        full: rx_fifo_full_internal,
        empty: rx_fifo_empty
    }

    // Internal signals for TX path.
    signal tx_state: nat[3]
    signal tx_baud_counter: nat[COUNTER_WIDTH]
    signal tx_bit_index: nat[BIT_COUNT_WIDTH]
    signal tx_shift_reg: bit[DATA_BITS]

    // Forward-declared combinational signals for FIFO control.
    signal tx_fifo_read: bit
    signal tx_fifo_data: bit[DATA_BITS]
    signal tx_fifo_empty_internal: bit

    // Internal signals for RX path.
    signal rx_state: nat[3]
    signal rx_baud_counter: nat[COUNTER_WIDTH]
    signal rx_bit_index: nat[BIT_COUNT_WIDTH]
    signal rx_shift_reg: bit[DATA_BITS]
    signal rx_byte_valid: bit
    signal rx_byte_data: bit[DATA_BITS]
    signal rx_fifo_full_internal: bit

    // Edge detection for RX serial line.
    signal rx_serial_prev: bit

    // TX state machine constants.
    const TX_IDLE: nat[3]  = 0
    const TX_START: nat[3] = 1
    const TX_DATA: nat[3]  = 2
    const TX_STOP: nat[3]  = 3

    // RX state machine constants.
    const RX_IDLE: nat[3]  = 0
    const RX_START: nat[3] = 1
    const RX_DATA: nat[3]  = 2
    const RX_STOP: nat[3]  = 3

    // ── TX Sequential Logic ─────────────────────────────────

    on(clk.rise) {
        if rst {
            tx_state = TX_IDLE
            tx_baud_counter = 0
            tx_bit_index = 0
            tx_shift_reg = 0
        } else {
            match tx_state {
                TX_IDLE => {
                    tx_serial = 1  // Line idle high
                    if !tx_fifo_empty_internal {
                        tx_shift_reg = tx_fifo_data
                        tx_fifo_read = 1
                        tx_state = TX_START
                        tx_baud_counter = 0
                    }
                },
                TX_START => {
                    tx_serial = 0  // Start bit
                    if tx_baud_counter == CYCLES_PER_BIT - 1 {
                        tx_baud_counter = 0
                        tx_bit_index = 0
                        tx_state = TX_DATA
                    } else {
                        tx_baud_counter = tx_baud_counter + 1
                    }
                },
                TX_DATA => {
                    tx_serial = tx_shift_reg[0]
                    if tx_baud_counter == CYCLES_PER_BIT - 1 {
                        tx_baud_counter = 0
                        tx_shift_reg = tx_shift_reg >> 1
                        if tx_bit_index == DATA_BITS - 1 {
                            tx_state = TX_STOP
                        } else {
                            tx_bit_index = tx_bit_index + 1
                        }
                    } else {
                        tx_baud_counter = tx_baud_counter + 1
                    }
                },
                TX_STOP => {
                    tx_serial = 1  // Stop bit
                    if tx_baud_counter == CYCLES_PER_BIT - 1 {
                        tx_state = TX_IDLE
                    } else {
                        tx_baud_counter = tx_baud_counter + 1
                    }
                }
            }
        }
    }

    // ── RX Sequential Logic ─────────────────────────────────

    on(clk.rise) {
        if rst {
            rx_state = RX_IDLE
            rx_baud_counter = 0
            rx_bit_index = 0
            rx_shift_reg = 0
            rx_byte_valid = 0
            rx_serial_prev = 1
        } else {
            rx_serial_prev = rx_serial
            rx_byte_valid = 0  // Default: pulse for one cycle only

            match rx_state {
                RX_IDLE => {
                    // Detect falling edge on rx_serial (start bit).
                    if rx_serial_prev & !rx_serial {
                        rx_state = RX_START
                        rx_baud_counter = 0
                    }
                },
                RX_START => {
                    // Sample at mid-bit to confirm start bit is still low.
                    if rx_baud_counter == HALF_BIT - 1 {
                        if !rx_serial {
                            rx_baud_counter = 0
                            rx_bit_index = 0
                            rx_state = RX_DATA
                        } else {
                            // False start — go back to idle.
                            rx_state = RX_IDLE
                        }
                    } else {
                        rx_baud_counter = rx_baud_counter + 1
                    }
                },
                RX_DATA => {
                    if rx_baud_counter == CYCLES_PER_BIT - 1 {
                        rx_baud_counter = 0
                        // Shift in from MSB side; after DATA_BITS shifts,
                        // the first bit received is in position [0].
                        rx_shift_reg = (rx_serial << (DATA_BITS - 1))
                                     | (rx_shift_reg >> 1)
                        if rx_bit_index == DATA_BITS - 1 {
                            rx_state = RX_STOP
                        } else {
                            rx_bit_index = rx_bit_index + 1
                        }
                    } else {
                        rx_baud_counter = rx_baud_counter + 1
                    }
                },
                RX_STOP => {
                    if rx_baud_counter == CYCLES_PER_BIT - 1 {
                        if rx_serial {
                            // Valid stop bit — emit the received byte.
                            rx_byte_data = rx_shift_reg
                            rx_byte_valid = 1
                        }
                        rx_state = RX_IDLE
                    } else {
                        rx_baud_counter = rx_baud_counter + 1
                    }
                }
            }
        }
    }

    // ── Combinational Outputs ───────────────────────────────

    tx_ready = !tx_fifo_full
    rx_valid = !rx_fifo_empty
}
```

### What Changed from Earlier Chapters

Compare this to the UART you built in Chapters 2-4. The logic is identical. The state machines are the same. The FIFO wiring is the same. But every magic number is gone:

- `434` became `CYCLES_PER_BIT`, which is `CLK_FREQ_HZ / BAUD_RATE`.
- `217` became `HALF_BIT`, which is `CYCLES_PER_BIT / 2`.
- `bit[9]` for the baud counter became `nat[COUNTER_WIDTH]` where `COUNTER_WIDTH = clog2(CYCLES_PER_BIT)`.
- `bit[8]` for data became `bit[DATA_BITS]`.
- `FIFO<8, 16>` became `FIFO<DATA_BITS, FIFO_DEPTH>`.

The const declarations at the top of the impl form a chain: `CLK_FREQ_HZ` and `BAUD_RATE` are generics from the entity, `CYCLES_PER_BIT` is derived from those, `HALF_BIT` is derived from `CYCLES_PER_BIT`, and `COUNTER_WIDTH` is derived from `CYCLES_PER_BIT` via `clog2()`. All of these are resolved at compile time. None of them exist as signals in the generated hardware.

### Test vs. Production Instantiation

This is where parameterization pays off immediately. In production, you instantiate the UART with real-world timing:

```
// Production configuration: 50 MHz clock, 115200 baud, 8-bit data.
// CYCLES_PER_BIT = 50_000_000 / 115200 = 434.
// A single byte takes ~4340 clock cycles to transmit.
let uart = UartTop<50_000_000, 115200, 8, 16> {
    clk: sys_clk,
    rst: sys_rst,
    tx_data: cpu_tx_data,
    tx_valid: cpu_tx_valid,
    tx_ready: cpu_tx_ready,
    rx_data: cpu_rx_data,
    rx_valid: cpu_rx_valid,
    rx_read: cpu_rx_read,
    tx_serial: uart_txd,
    rx_serial: uart_rxd,
    tx_fifo_full: status_tx_full,
    rx_fifo_empty: status_rx_empty
}
```

In simulation, you want the same logic but with timing that does not waste millions of cycles waiting for a single byte:

```
// Simulation configuration: fast timing for testbench.
// CLK_FREQ_HZ = 1000, BAUD_RATE = 100 -> CYCLES_PER_BIT = 10.
// A single byte takes ~100 clock cycles — 43x faster than production.
// FIFO_DEPTH = 4 to keep simulation memory small.
let uart_sim = UartTop<1000, 100, 8, 4> {
    clk: test_clk,
    rst: test_rst,
    tx_data: test_tx_data,
    tx_valid: test_tx_valid,
    tx_ready: test_tx_ready,
    rx_data: test_rx_data,
    rx_valid: test_rx_valid,
    rx_read: test_rx_read,
    tx_serial: loopback_serial,
    rx_serial: loopback_serial,
    tx_fifo_full: open,
    rx_fifo_empty: open
}
```

The logic inside `UartTop` is identical in both cases. The compiler produces two different modules: one where `CYCLES_PER_BIT` is 434 and counters are 9 bits wide, and another where `CYCLES_PER_BIT` is 10 and counters are 4 bits wide. Same source, different hardware, both correct.

This pattern — test parameterization — is one of the most practical benefits of const generics. In SystemVerilog, you would need to either recompile with different `define` values, pass parameters through the hierarchy, or wait through millions of simulation cycles. In skalp, it is just a different instantiation.

### Using Defaults for Clean Instantiation

Because every generic parameter has a default, you can omit parameters you do not need to change:

```
// All defaults: 50 MHz, 115200 baud, 8-bit, FIFO depth 16.
let uart_default = UartTop {
    clk: sys_clk,
    rst: sys_rst,
    // ... ports
}

// Override only clock frequency and baud rate.
// DATA_BITS and FIFO_DEPTH use their defaults (8, 16).
let uart_fast = UartTop<100_000_000, 921600> {
    clk: fast_clk,
    rst: sys_rst,
    // ... ports
}
```

When you omit the angle brackets entirely, all parameters take their defaults. When you provide partial values, they fill in left to right and the rest use defaults. This keeps instantiation sites clean when the defaults are sensible for most use cases.

---

## Build and Test

Your project structure should now look like this:

```
uart-tutorial/
  skalp.toml
  src/
    counter.sk       (Chapter 1)
    uart_tx.sk        (Chapter 2)
    uart_rx.sk        (Chapter 3)
    fifo.sk           (Chapter 4)
    uart_top.sk       (updated — now fully parameterized)
    adder.sk          (this chapter's standalone example)
```

Update `skalp.toml` to set the top entity:

```toml
[package]
name = "uart-tutorial"
version = "0.1.0"

[build]
top = "UartTop"
```

Build the parameterized design:

```bash
skalp build
```

You should see all entities compile, with the parameterized UART elaborated at its default parameters. The generated SystemVerilog in `build/uart_top.sv` will have `CYCLES_PER_BIT = 434` inlined as a literal — no parameter at the SV level, because skalp resolves everything at compile time.

To simulate with fast timing, use the `--params` flag:

```bash
skalp sim --entity UartTop \
    --params "CLK_FREQ_HZ=1000,BAUD_RATE=100,FIFO_DEPTH=4" \
    --cycles 500 \
    --vcd build/uart_fast.vcd
```

This instantiates the UART with simulation-friendly timing. A full TX/RX byte transfer completes in about 100 cycles instead of 4340, so 500 cycles is enough to see multiple bytes transmitted and received.

Compare with production timing:

```bash
skalp sim --entity UartTop --cycles 10000 --vcd build/uart_prod.vcd
```

Open both VCD files in a waveform viewer. The waveforms will look identical in shape — same state transitions, same FIFO behavior — but the fast version compresses time by a factor of 43.

---

## Quick Reference

| Concept | Syntax | Example |
|---------|--------|---------|
| Generic parameter | `<const NAME: nat>` | `entity Adder<const WIDTH: nat>` |
| Generic with default | `<const NAME: nat = value>` | `<const WIDTH: nat = 8>` |
| Multiple generics | `<const A: nat, const B: nat>` | `<const WIDTH: nat = 8, const DEPTH: nat = 16>` |
| Explicit instantiation | `Entity<value>` | `Adder<16> { ... }` |
| Default instantiation | `Entity { ... }` | `Adder { ... }` uses WIDTH=8 |
| Const declaration | `const NAME: type = expr` | `const HALF_BIT: nat = CYCLES_PER_BIT / 2` |
| Compile-time log2 | `clog2(expr)` | `clog2(1024)` evaluates to 10 |
| Derived width | `bit[const_expr]` | `signal counter: nat[clog2(CYCLES_PER_BIT)]` |
| Const arithmetic | `+`, `-`, `*`, `/`, `<<`, `>>` | `const TOTAL: nat = WIDTH + ADDR_WIDTH` |
| Numeric literal separator | `_` in numbers | `50_000_000` |

---

## Next: Structs and Hierarchical Composition

The parameterized UART works, but the port list is getting long. `UartTop` already has twelve ports, and as you add features like parity, flow control, and error reporting, that number will grow. Flat port lists become a maintenance burden — you must update every instantiation site whenever a port changes.

In Chapter 6, you will learn how to group related ports into **structs** and use them as compound types on entity ports. Instead of separate `tx_data`, `tx_valid`, and `tx_ready` ports, you will define a `Stream` struct and use `in tx: Stream<DATA_BITS>` as a single port. You will also see how structs compose hierarchically, how to access fields with dot notation, and how struct-typed ports make instantiation sites cleaner and refactoring safer.

Continue to [Chapter 6: Structs and Hierarchical Composition](../06-structs-and-composition/).
