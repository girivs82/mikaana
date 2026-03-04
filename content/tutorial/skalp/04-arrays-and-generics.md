---
title: "Chapter 4: Arrays and Generics — FIFO Buffering"
date: 2025-07-15
summary: "Array types, generic parameters, and clog2() — build a parameterized FIFO and add buffering to the UART."
tags: ["skalp", "tutorial", "fifo", "generics", "arrays"]
weight: 4
ShowToc: true
aliases: ["/tutorial/04-arrays-and-generics/"]
---

## What You'll Learn

Your UART transmitter and receiver from Chapters 2 and 3 work, but they have no buffering. If the TX is busy serialising a byte and you try to send another, the new byte is lost. If the RX completes a byte and the consumer is not ready, it disappears. Real peripherals solve this with **FIFOs** (first-in, first-out buffers).

Building a FIFO is also the perfect vehicle for learning three important skalp features:

- **Array types** — `[T; N]` declares a fixed-size array of elements, used here as the FIFO storage.
- **Generic parameters** — `entity Name<const PARAM: nat = default>` lets you write one module that works at any width and depth.
- **`clog2()`** — a compile-time function that computes the number of bits needed to represent a value, used for pointer and counter widths.

By the end of this chapter, your UART will have TX and RX FIFOs, cleanly decoupling the application logic from the serial timing. Your running project will include:

- `src/fifo.sk` — a standalone parameterized synchronous FIFO
- `src/uart_buffered.sk` — the UART with TX and RX FIFOs integrated

---

## Standalone Example: Parameterized FIFO

A synchronous FIFO is a circular buffer with read and write pointers. You write data at the write pointer, read data at the read pointer, and each pointer wraps around when it reaches the end of the buffer. A separate count tracks how many elements are currently stored, which determines the full and empty flags.

### The Interface

Before looking at the implementation, let us study the entity declaration:

```skalp
entity FIFO<const WIDTH: nat = 8, const DEPTH: nat = 16> {
    in  clk:     clock
    in  rst:     reset
    in  wr_en:   bit
    in  wr_data: bit[WIDTH]
    in  rd_en:   bit
    out rd_data: bit[WIDTH]
    out full:    bit
    out empty:   bit
}
```

There is a lot packed into this declaration. Let us unpack it:

- `<const WIDTH: nat = 8, const DEPTH: nat = 16>` — these are **generic parameters**. `WIDTH` controls how many bits each entry has, `DEPTH` controls how many entries the FIFO can hold. Both have defaults (8 and 16), so you can instantiate a `FIFO` without specifying them and get an 8-bit, 16-deep buffer.
- `bit[WIDTH]` — the data ports use the generic parameter in their type. When you instantiate `FIFO<32, 64>`, `wr_data` and `rd_data` become 32-bit ports.
- `clog2()` is used internally for pointer widths (see below). The interface itself is simple: write data, read data, and full/empty status flags.

### The Implementation

Create `src/fifo.sk`:

```skalp
// fifo.sk — parameterized synchronous FIFO with full/empty flags

entity FIFO<const WIDTH: nat = 8, const DEPTH: nat = 16> {
    in clk: clock
    in rst: reset(active_high)
    in wr_en: bit
    in wr_data: bit[WIDTH]
    out full: bit
    in rd_en: bit
    out rd_data: bit[WIDTH]
    out empty: bit
}

impl FIFO {
    // Storage array — DEPTH entries, each WIDTH bits wide.
    // This is the actual memory that holds the buffered data.
    // Depending on the synthesis target and size, the tool may
    // infer this as registers, distributed RAM, or block RAM.
    signal memory: [bit[WIDTH]; DEPTH]

    // Write and read pointers. clog2(DEPTH) gives us just enough
    // bits to index every position in the array.
    //
    // For DEPTH = 16: clog2(16) = 4, so pointers are 4 bits
    //     (values 0..15 — exactly the valid indices).
    // For DEPTH = 32: clog2(32) = 5, so pointers are 5 bits
    //     (values 0..31).
    signal wr_ptr: nat[clog2(DEPTH)]
    signal rd_ptr: nat[clog2(DEPTH)]

    // Element count. Needs clog2(DEPTH + 1) bits because the count
    // ranges from 0 (empty) to DEPTH (full). For a 16-deep FIFO,
    // that's 0..16, which requires 5 bits — one more than the
    // pointer width. This extra bit is how we distinguish "full"
    // from "empty" (both have equal pointers, but different counts).
    signal count: nat[clog2(DEPTH+1)]

    // Combinational outputs — driven from registered state.
    // These are always valid and update every cycle based on
    // the current count.
    empty = (count == 0)
    full = (count == DEPTH)

    // Read data is combinational — always shows the element at
    // the current read pointer, even before rd_en is asserted.
    rd_data = memory[rd_ptr]

    on(clk.rise) {
        if rst {
            wr_ptr = 0
            rd_ptr = 0
            count = 0
        } else {
            // Write path: store data and advance pointer
            if wr_en && !full {
                memory[wr_ptr] = wr_data
                wr_ptr = (wr_ptr + 1) % DEPTH
            }

            // Read path: advance pointer (data is combinational)
            if rd_en && !empty {
                rd_ptr = (rd_ptr + 1) % DEPTH
            }

            // Count tracking: increment on write-only, decrement
            // on read-only. Simultaneous read+write leaves count
            // unchanged because neither condition is true.
            if wr_en && !rd_en && !full {
                count = count + 1
            } else if !wr_en && rd_en && !empty {
                count = count - 1
            }
        }
    }
}
```

### Anatomy of the FIFO

Let us examine each key feature in detail.

**Generic parameters.** The `<const WIDTH: nat = 8, const DEPTH: nat = 16>` syntax declares two compile-time constants with defaults. When you instantiate the FIFO, you can override either or both. The compiler substitutes the values before synthesis, producing specialised hardware — there is no runtime cost. A `FIFO<8, 16>` produces a design with an 8-bit-wide, 16-deep memory. A `FIFO<32, 256>` produces a completely different design with a 32-bit-wide, 256-deep memory and wider pointers.

Generic parameters in skalp are true type-level constants. They can appear in type expressions (`bit[WIDTH]`), in `clog2()` arguments, in comparison values (`DEPTH`), and anywhere else a compile-time constant is valid. The compiler evaluates all generic expressions at compile time and checks that the resulting types are consistent.

**Array type.** `[bit[WIDTH]; DEPTH]` is an array of `DEPTH` elements, each of type `bit[WIDTH]`. The syntax reads as "an array of DEPTH things of type bit[WIDTH]." This maps to a register file or block RAM depending on the synthesis target and the array size.

You index into the array with `memory[wr_ptr]`, and you assign individual elements with `memory[wr_ptr] = wr_data`. The index must be of a type wide enough to cover all valid indices — here, `nat[clog2(DEPTH)]` guarantees this.

Arrays in skalp are fixed-size. The size is a compile-time constant (it can use generic parameters and `clog2()`). There are no dynamically-sized arrays — this is hardware, and the size of every memory must be known at synthesis time.

**`clog2()`.** This function computes the ceiling of log-base-2 at compile time. Some examples:

| Expression | Value | Why |
|---|---|---|
| `clog2(16)` | 4 | 2^4 = 16, so 4 bits cover indices 0..15 |
| `clog2(17)` | 5 | 2^4 = 16 < 17, so we need 5 bits for 0..16 |
| `clog2(1)` | 0 | A single-element "array" needs 0 index bits |
| `clog2(DEPTH)` | varies | Adapts to the generic parameter |
| `clog2(DEPTH + 1)` | varies | One extra bit for the count (0 to DEPTH inclusive) |

The result of `clog2()` is used directly in type declarations like `nat[clog2(DEPTH)]` — the pointer width adapts automatically to the FIFO depth. You never need to manually compute the width or define a separate constant for it.

**Pointer wrapping with modulo.** The expression `(wr_ptr + 1) % DEPTH` advances the pointer and wraps it back to 0 when it reaches the end. For a 16-deep FIFO, when `wr_ptr` is 15, `(15 + 1) % 16 = 0`. This works for any depth, not just powers of two — you can have a 17-deep or 100-deep FIFO.

For power-of-two depths, the synthesiser recognises the modulo operation and optimises it to simple bit truncation, producing the same hardware as a bitmask approach. For non-power-of-two depths, it generates a comparator and mux.

**Combinational read.** The line `rd_data = memory[rd_ptr]` is a combinational assignment outside the `on` block. This means `rd_data` always shows the element at the current read pointer, even before `rd_en` is asserted. When `rd_en` pulses, the `on` block advances `rd_ptr`, and `rd_data` combinationally updates to show the next element on the following cycle. This "read-before-advance" protocol is simple and efficient.

**Forward references.** Notice that `full` and `empty` are used inside the `on` block (in the guard conditions `!full` and `!empty`) but the combinational assignments `full = (count == DEPTH)` and `empty = (count == 0)` appear *above* the `on` block. The position does not matter — skalp resolves the dependency graph regardless of textual order. You can organise your code logically without fighting the language.

**Simultaneous read and write.** When both `wr_en` and `rd_en` are asserted in the same cycle, both the write and read paths execute independently. Both pointers advance. The count update logic handles this implicitly: `wr_en && !rd_en` is false and `!wr_en && rd_en` is also false, so the count stays unchanged. This is correct — one element in, one element out, net change is zero. No explicit simultaneous-case handler is needed.

---

> **Coming from SystemVerilog?**
>
> The FIFO maps closely to what you would write in SV, but there are a few notable differences:
>
> - **Array syntax is inverted.** In skalp, `[bit[8]; 16]` means "16 elements of 8-bit vectors." In SV, the equivalent is `logic [7:0] mem [0:15]` — type first with the bit range, then the array dimension. Skalp puts the element type inside the brackets and the count outside, which reads more naturally as "an array of N things of type T." If you have used Rust, this syntax will feel familiar.
> - **`clog2()` vs `$clog2()`.** Both compute the same value, but skalp's `clog2()` is a compile-time type-level function. You use it directly in type declarations: `nat[clog2(DEPTH)]`. There is no `$` prefix and no need for a separate `localparam` to hold the result. The compiler evaluates it during type checking, not during elaboration.
> - **No `[WIDTH-1:0]` pattern.** In SV, `logic [7:0]` means bits 7 down to 0 — an 8-bit vector. In skalp, `bit[8]` means an 8-bit vector. The width is the natural number, not a range. You never write `-1` in a type declaration. This eliminates a common source of off-by-one bugs: `logic [WIDTH:0]` vs `logic [WIDTH-1:0]` — in skalp, you just write `bit[WIDTH]` or `bit[WIDTH + 1]`.
> - **Generic params vs `parameter`.** SV uses `parameter` inside the module or `#(parameter ...)` in the port list. Skalp uses `<const NAME: type = default>` on the entity declaration. The semantics are the same — compile-time constants that specialise the module — but skalp's generics are part of the type system, so a `FIFO<8, 16>` and a `FIFO<32, 64>` are distinct types. You cannot accidentally connect a 32-bit FIFO's output to an 8-bit consumer without the compiler catching it.
> - **No sensitivity lists.** In SV, `always_comb` requires the tool to infer the sensitivity list, and incomplete lists are a common bug source. In skalp, combinational assignments are just expressions outside `on` blocks — the compiler knows every dependency.

---

## Running Project: Adding FIFOs to the UART

With the FIFO module ready, we can buffer both the TX and RX paths. The updated architecture looks like this:

```
                  ┌──────────┐     ┌──────────┐
  tx_data_in ──>  │  TX FIFO │ ──> │  UartTx  │ ──> tx (pin)
  tx_write   ──>  │          │     │          │
                  └──────────┘     └──────────┘

                  ┌──────────┐     ┌──────────┐
  rx_data_out <── │  RX FIFO │ <── │  UartRx  │ <── rx (pin)
  rx_read     ──> │          │     │          │
                  └──────────┘     └──────────┘
```

The application writes bytes into the TX FIFO. When the FIFO is not empty and the transmitter is idle, the UART controller pops a byte and feeds it to the transmitter. On the receive side, when the receiver completes a byte, it is pushed into the RX FIFO. The application reads bytes out of the RX FIFO at its own pace.

### The Buffered UART

Create `src/uart_buffered.sk`:

```skalp
// uart_buffered.sk — UART with TX and RX FIFOs

use fifo::FIFO;
use uart_tx::UartTx;
use uart_rx::UartRx;

entity UartBuffered {
    in  clk:          clock
    in  rst:          reset

    // TX interface — application writes bytes here
    in  tx_data_in:   bit[8]
    in  tx_write:     bit
    out tx_full:      bit

    // RX interface — application reads bytes here
    out rx_data_out:  bit[8]
    in  rx_read:      bit
    out rx_empty:     bit

    // Serial pins
    out tx:           bit
    in  rx:           bit
}

impl UartBuffered {
    // Internal signals for FIFO-to-UART wiring.
    // These connect the FIFO outputs to the UART inputs and vice versa.
    signal tx_fifo_data:  bit[8]
    signal tx_fifo_empty: bit
    signal tx_fifo_read:  bit
    signal tx_busy:       bit

    signal rx_byte:       bit[8]
    signal rx_valid:      bit
    signal rx_fifo_full:  bit

    // TX FIFO: 8 bits wide, 16 entries deep.
    // The application writes into this FIFO, and the UART TX
    // reads from it automatically.
    let tx_fifo = FIFO<8, 16> {
        clk:     clk,
        rst:     rst,
        wr_en:   tx_write,
        wr_data: tx_data_in,
        rd_en:   tx_fifo_read
    }

    tx_fifo_data  = tx_fifo.rd_data
    tx_full       = tx_fifo.full
    tx_fifo_empty = tx_fifo.empty

    // RX FIFO: 8 bits wide, 16 entries deep.
    // The UART RX writes into this FIFO when a byte is received,
    // and the application reads from it at its leisure.
    let rx_fifo = FIFO<8, 16> {
        clk:     clk,
        rst:     rst,
        wr_en:   rx_valid && !rx_fifo_full,
        wr_data: rx_byte,
        rd_en:   rx_read
    }

    rx_data_out  = rx_fifo.rd_data
    rx_fifo_full = rx_fifo.full
    rx_empty     = rx_fifo.empty

    // UART transmitter — reads from the TX FIFO.
    // tx_fifo_read serves as both the FIFO read enable and the
    // UART transmit enable — when we pop a byte from the FIFO,
    // we simultaneously start transmitting it.
    let uart_tx = UartTx {
        clk:      clk,
        rst:      rst,
        tx_data:  tx_fifo_data,
        tx_start: tx_fifo_read
    }

    tx = uart_tx.tx
    tx_busy = uart_tx.tx_busy

    // UART receiver — writes to the RX FIFO.
    // When rx_valid pulses, the byte on rx_byte is captured by
    // the RX FIFO (if not full).
    let uart_rx = UartRx {
        clk:      clk,
        rst:      rst,
        rx:       rx
    }

    rx_byte  = uart_rx.rx_data
    rx_valid = uart_rx.rx_valid

    // TX FIFO read controller: pop a byte when the transmitter
    // is idle and the FIFO has data. This single combinational
    // expression is all the "glue logic" needed to connect the
    // FIFO to the transmitter.
    tx_fifo_read = !tx_fifo_empty && !tx_busy
}
```

### How the Pieces Fit Together

**TX path.** The application asserts `tx_write` with `tx_data_in` to push a byte into the TX FIFO. The combinational expression `tx_fifo_read = !tx_fifo_empty && !tx_busy` automatically triggers a read from the FIFO whenever the transmitter is idle and data is available. The FIFO's `rd_data` output (accessed via `tx_fifo.rd_data`) feeds into `UartTx`'s `tx_data` port through the `tx_fifo_data` signal, and `tx_fifo_read` serves double duty as the transmitter's `tx_start` — starting transmission of the byte in the same cycle it is read from the FIFO.

This means bytes flow through the system automatically: the application pushes them into the FIFO, and the transmitter pulls them out one at a time as fast as the baud rate allows. The application never needs to check whether the transmitter is busy — it just writes to the FIFO and checks `tx_full` to avoid overflow.

**RX path.** When `UartRx` completes a byte, it pulses `rx_valid` for one cycle (as we designed in Chapter 3). The RX FIFO's `wr_en` is `rx_valid && !rx_fifo_full` — it captures the byte unless the FIFO is full. If the FIFO *is* full when a byte arrives, the byte is dropped. A production design might set an overflow flag here.

The application reads bytes out by asserting `rx_read`, and checks `rx_empty` to know whether data is available. The FIFO's `rd_data` output always holds the oldest unread byte.

**Output access with dot notation.** Sub-entity outputs are accessed with `instance.port` syntax: `tx_fifo.rd_data`, `uart_tx.tx`, `uart_rx.rx_data`. These are wired to internal signals or output ports with combinational assignments. This keeps the `let` bindings focused on input connections and puts the output wiring in explicit, readable assignments.

**The glue line.** The most important line in the entire module is the last one:

```skalp
tx_fifo_read = !tx_fifo_empty && !tx_busy
```

This single combinational expression is the "controller" that connects the FIFO to the transmitter. It says: "whenever there is data to send and the transmitter is not busy, start sending." No state machine, no additional logic — just a Boolean expression. This is the power of breaking a design into well-defined sub-modules with clean interfaces.

### Generic Instantiation Syntax

The instantiation `FIFO<8, 16>` passes generic arguments by position, matching `WIDTH = 8` and `DEPTH = 16`. You can also use the defaults:

```skalp
// Uses WIDTH = 8, DEPTH = 16 (both defaults)
let default_fifo = FIFO { clk: clk, rst: rst, ... }

// Override only DEPTH, WIDTH uses default of 8
let deep_fifo = FIFO<8, 256> { clk: clk, rst: rst, ... }

// 32-bit wide, 64 entries deep
let wide_fifo = FIFO<32, 64> { clk: clk, rst: rst, ... }
```

Each instantiation produces distinct hardware. `FIFO<8, 16>` and `FIFO<32, 64>` are completely independent — different memory sizes, different pointer widths, different count widths. The compiler computes `clog2()` for each set of parameters and generates the appropriate bit widths. At synthesis time, there is no "generic" FIFO — there are only the concrete, specialised versions.

### Why Count-Based, Not Pointer-Difference?

You might wonder why the FIFO uses a separate `elem_count` register instead of deriving fullness from the pointer difference (`wr_ptr - rd_ptr`). Both approaches work, but they have different trade-offs:

**Count-based** (our approach):
- Extra register (`count`), costs `clog2(DEPTH + 1)` flip-flops
- Full and empty are simple comparisons: `count == DEPTH`, `count == 0`
- Works for any DEPTH, not just powers of two
- The count value is directly available for diagnostics if needed

**Pointer-difference:**
- No extra register — derive count from `wr_ptr - rd_ptr`
- Requires power-of-two DEPTH for the subtraction to wrap correctly (or an extra MSB on each pointer)
- Full vs empty ambiguity when pointers are equal: is it 0 elements or DEPTH elements?
- Common SV solution: add one extra bit to each pointer, compare the MSB to distinguish full from empty

Both are valid. The count-based approach is simpler to understand and works for arbitrary depths, which is why we use it here. A power-of-two optimised FIFO might use the pointer-difference approach to save a few flip-flops.

---

## Build and Test

Compile the FIFO standalone:

```bash
skalp build src/fifo.sk
```

Compile the buffered UART (pulls in the FIFO, UartTx, and UartRx):

```bash
skalp build src/uart_buffered.sk
```

Run the test suite:

```bash
cargo test --test ch04_test
```

Verify in the waveform viewer:

1. Write several bytes in quick succession to `tx_data_in` — confirm they are buffered in the TX FIFO and transmitted one at a time.
2. Watch `tx_count` decrement as each byte finishes transmitting.
3. Confirm that `tx_fifo_read` pulses high at the exact moment between transmissions — when the transmitter finishes one byte and the FIFO still has data.
4. Send multiple bytes on the `rx` line — confirm they appear in the RX FIFO and `rx_count` increments.
5. Read bytes from the RX FIFO with `rx_read` — confirm `rx_data_out` produces them in FIFO order (first written, first read) and `rx_empty` goes high when the last byte is consumed.
6. Try to overfill the TX FIFO — write 17 or more bytes and confirm `tx_full` goes high after 16 writes and subsequent writes are ignored (the count stays at 16).
7. Test simultaneous read and write: while the TX is busy transmitting and the FIFO has some entries, write another byte. Confirm the count changes correctly.

---

## Testing Your Design

FIFOs have structural invariants — the count must always match the number of writes minus reads, `empty` must be set when count is zero, and `full` must be set when count equals depth. Testbenches for data structures should verify these invariants, not just input/output values.

Here are representative tests from `tests/ch04_test.rs`:

### FIFO

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_fifo_empty_after_reset() {
    let mut tb = Testbench::with_top_module("src/fifo.sk", "FIFO")
        .await.unwrap();
    tb.reset(2).await;

    tb.expect("empty", 1u32).await;
    tb.expect("full", 0u32).await;
}

#[tokio::test]
async fn test_fifo_write_and_read_ordering() {
    let mut tb = Testbench::with_top_module("src/fifo.sk", "FIFO")
        .await.unwrap();
    tb.reset(2).await;

    // Write 5 bytes: "Hello"
    let test_data: [u8; 5] = [0x48, 0x65, 0x6C, 0x6C, 0x6F];
    for &byte in &test_data {
        tb.set("wr_en", 1u8);
        tb.set("wr_data", byte as u32);
        tb.clock(1).await;
    }
    tb.set("wr_en", 0u8);

    // Read them back — FIFO should preserve order
    // rd_data is combinational: valid before pulsing rd_en
    for (i, &expected) in test_data.iter().enumerate() {
        let rd_data = tb.get_u64("rd_data").await;
        assert_eq!(rd_data, expected as u64,
            "FIFO byte {}: expected 0x{:02X}, got 0x{:02X}",
            i, expected, rd_data);

        // Pulse rd_en to advance read pointer
        tb.set("rd_en", 1u8);
        tb.clock(1).await;
        tb.set("rd_en", 0u8);
    }
}

#[tokio::test]
async fn test_fifo_full_flag() {
    let mut tb = Testbench::with_top_module("src/fifo.sk", "FIFO")
        .await.unwrap();
    tb.reset(2).await;

    // Write 16 entries (default DEPTH = 16)
    for i in 0..16u32 {
        tb.set("wr_en", 1u8);
        tb.set("wr_data", i);
        tb.clock(1).await;
    }
    tb.set("wr_en", 0u8);

    tb.expect("full", 1u32).await;
    tb.expect("empty", 0u32).await;
}
```

### UartBuffered

```rust
#[tokio::test]
async fn test_uart_buffered_rx_to_fifo() {
    let mut tb = Testbench::with_top_module("src/uart_buffered.sk", "UartBuffered")
        .await.unwrap();
    tb.reset(2).await;
    tb.set("rx", 1u8);
    tb.clock(10).await;

    // Drive a byte onto the RX pin
    drive_rx_byte(&mut tb, 0xA3).await;
    tb.clock(5).await;

    // RX FIFO should have data
    tb.expect("rx_empty", 0u32).await;

    // rd_data is combinational — read before pulsing rd_en
    let received = tb.get_u64("rx_data_out").await;
    tb.set("rx_read", 1u8);
    tb.clock(1).await;
    tb.set("rx_read", 0u8);

    assert_eq!(received, 0xA3);
}
```

Run with:

```bash
cargo test
```

**Exercise:** Write a `test_fifo_pointer_wrap` test that writes and reads 20 entries (more than the FIFO depth of 16) to verify the circular buffer pointers wrap correctly.

---

## Quick Reference

| Concept | Syntax | Example |
|---|---|---|
| Array type | `[T; N]` | `signal memory: [bit[8]; 16]` |
| Array indexing | `array[index]` | `memory[wr_ptr]` |
| Array element write | `array[index] = value` | `memory[wr_ptr] = wr_data` |
| Generic entity | `entity Name<const P: type = default>` | `entity FIFO<const WIDTH: nat = 8, const DEPTH: nat = 16>` |
| Generic instantiation | `Entity<args>` | `FIFO<8, 16> { ... }` |
| Compile-time log2 | `clog2(expr)` | `nat[clog2(DEPTH)]` |
| Pointer width from depth | `nat[clog2(N)]` | `signal wr_ptr: nat[clog2(DEPTH)]` |
| Count width (extra bit) | `nat[clog2(N + 1)]` | `signal count: nat[clog2(DEPTH + 1)]` |
| Modulo pointer wrap | `(ptr + 1) % DEPTH` | `wr_ptr = (wr_ptr + 1) % DEPTH` |
| Forward reference | Use before assign (combinational) | `full` used in `on`, assigned above |
| Default generic values | Omit args to use defaults | `FIFO { ... }` uses WIDTH=8, DEPTH=16 |

---

## Next

Your UART now has buffering, but both the transmitter and receiver have hardcoded parameters: 50 MHz clock, 115200 baud, 8 data bits. If you want to target a different FPGA or baud rate, you need to edit the source. The FIFO depth is also hardcoded at 16 — what if you need deeper buffers for a high-throughput application, or shallower ones to save resources?

Right now, changing any of these parameters means editing multiple files and recalculating constants by hand. There is a better way.

In **[Chapter 5: Const Generics and Parameterization](../05-parameterization/)**, you will make the entire UART configurable — clock frequency, baud rate, FIFO depth — using generic parameters and compile-time computation. You will learn how constants like `CYCLES_PER_BIT` can be computed from higher-level parameters like `CLK_FREQ` and `BAUD_RATE`, so you specify the intent and let the compiler do the arithmetic. You will also learn how to use different parameter sets for simulation versus synthesis, avoiding the pain of waiting for real-time baud periods in test.
