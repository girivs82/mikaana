---
title: "Chapter 8: Clock Domain Crossing"
date: 2025-07-15
summary: "Clock domain lifetimes, CDC compile-time safety, dual-clock entities, 2-flop synchronizers, Gray code pointers, and async FIFOs — make the UART dual-clock with compile-time guarantees against metastability bugs."
tags: ["skalp", "tutorial", "hardware", "hdl"]
weight: 8
ShowToc: true
---

## What This Chapter Teaches

Every design we have built so far lives in a single clock domain. One `clk: clock` drives everything. In the real world, that is the exception. A UART peripheral on an SoC will typically have at least two clocks: a system bus clock for register access and a baud-rate-derived clock for the serial interface. More complex designs have dozens of domains — PCIe at 250 MHz, DDR at 400 MHz, video at 148.5 MHz, a slow management bus at 25 MHz — all coexisting on the same die.

When a signal crosses from one clock domain to another without proper synchronization, the receiving flip-flop can enter a **metastable** state — it settles to neither 0 nor 1for an unpredictable amount of time. This is not a logic error. It is an electrical phenomenon that corrupts data, crashes state machines, and produces failures that appear once in a million cycles under specific temperature and voltage conditions. CDC bugs are among the most expensive defects in silicon — they are almost impossible to reproduce in simulation and often escape to production.

skalp eliminates this class of bugs at **compile time**. Every clock signal carries a **lifetime** — a compile-time marker that identifies its domain. Every signal inherits a domain from the `on(clk.rise)` block that assigns it. If you try to read a signal from domain `'a` inside an `on` block clocked by domain `'b`, the compiler rejects the code with a clear error message — unless the assignment happens through a properly annotated synchronizer.

By the end of this chapter you will understand:

- How `clock<'domain>` declares a clock with a named lifetime
- How signals acquire clock domain membership from their `on` block
- Why direct cross-domain reads produce a compile error
- How to build a 2-flop synchronizer entity for single-bit signals
- How Gray code encoding makes multi-bit pointer synchronization safe
- How to build an asynchronous FIFO with Gray code pointers
- How `#[cdc]` annotations declare and verify synchronization intent
- How to restructure the UART as a dual-clock design with async FIFOs at the domain boundaries

These are the tools that replace a $50,000-per-seat CDC verification tool with a single `skalp build`.

---

## Standalone Example: 2-Flop Synchronizer

The simplest CDC primitive is the **double-flop synchronizer**: two flip-flops in series, both clocked by the destination domain. The first flop samples the asynchronous input and may go metastable. The second flop samples the first, and because metastability resolves exponentially fast, the probability of the second flop also going metastable is negligibly small (on the order of 10^-20 per clock cycle in modern processes).

This circuit is safe only for **single-bit** signals that change slowly relative to the destination clock. Multi-bit buses need Gray code encoding or handshake protocols — we will build those next.

Create `src/synchronizer.sk`:

```
// synchronizer.sk — 2-flop CDC synchronizer for single-bit signals
//
// The 'src and 'dst lifetime parameters declare that this entity
// bridges two clock domains. The compiler uses these lifetimes to
// verify that:
//   1. data_in belongs to the 'src domain
//   2. data_out belongs to the 'dst domain
//   3. The cross-domain assignment is contained within this entity

entity Synchronizer<'src, 'dst> {
    in  clk_dst:  clock<'dst>,
    in  rst:      reset,
    in  data_in:  bit<'src>,
    out data_out: bit<'dst>
}

impl Synchronizer {
    signal sync_ff1: bit
    signal sync_ff2: bit

    on(clk_dst.rise) {
        if rst {
            sync_ff1 = 0
            sync_ff2 = 0
        } else {
            sync_ff1 = data_in    // first flop samples async input
            sync_ff2 = sync_ff1   // second flop reduces metastability
        }
    }

    data_out = sync_ff2
}
```

### What Makes This Different From SystemVerilog

In SystemVerilog, a 2-flop synchronizer looks almost identical — two `always_ff` flops in series. The difference is not in the circuit but in the **guarantees**.

In SystemVerilog, nothing prevents you from accidentally reading `data_in` directly somewhere else in the design, bypassing the synchronizer. Nothing prevents you from connecting `data_in` to a signal from the wrong clock domain. Nothing checks that `sync_ff1` and `sync_ff2` are actually clocked by the destination clock. The correctness of CDC is maintained entirely by convention, code review, and expensive third-party tools that run after the design is complete.

In skalp, the `<'src, 'dst>` lifetime parameters are part of the type system. The compiler tracks which domain every signal belongs to and rejects any cross-domain assignment that does not pass through a synchronizer entity. The check happens on every build, not as a separate verification step.

### What Happens Without Lifetimes

Here is a CDC bug that compiles without warnings in SystemVerilog:

```
// BUG: direct cross-domain assignment without synchronization.
// In SystemVerilog, this compiles and simulates perfectly.
// In skalp, this is a compile error.

entity BrokenDesign<'fast, 'slow> {
    in  fast_clk: clock<'fast>,
    in  slow_clk: clock<'slow>,
    in  rst:      reset,
    in  fast_data: bit[8]<'fast>,
    out slow_reg:  bit[8]<'slow>
}

impl BrokenDesign {
    signal captured: bit[8]

    on(slow_clk.rise) {
        if rst {
            captured = 0
        } else {
            captured = fast_data    // ERROR: clock domain mismatch
        }
    }

    slow_reg = captured
}
```

The compiler output:

```
error[E0401]: clock domain crossing without synchronization
  --> src/broken.sk:19:24
   |
19 |             captured = fast_data
   |                        ^^^^^^^^^ signal `fast_data` belongs to clock domain 'fast
   |
   = note: `captured` is assigned in an `on(slow_clk.rise)` block (domain 'slow)
   = note: reading a 'fast signal in a 'slow context requires a synchronizer
   = help: use `synchronize(fast_data)` or route through a Synchronizer entity
```

This error message tells you exactly what is wrong, which domains are involved, and how to fix it. The bug never reaches simulation, let alone silicon.

### Fixing the Bug

There are two ways to synchronize. For single-bit signals, instantiate the `Synchronizer` entity:

```
let synced = Synchronizer<'fast, 'slow> {
    clk_dst:  slow_clk,
    rst:      rst,
    data_in:  fast_data[0],    // single-bit only
    data_out: synced_bit
}
```

For multi-bit data, you need an async FIFO (built in the next section) or a handshake protocol. The `synchronize()` built-in is a shorthand for the 2-flop synchronizer on single-bit signals:

```
signal synced_flag: bit<'slow>

on(slow_clk.rise) {
    synced_flag = synchronize(fast_flag)    // built-in 2-flop sync
}
```

The compiler verifies that `synchronize()` is only applied to single-bit signals. Attempting to synchronize a multi-bit bus produces a separate error:

```
error[E0402]: synchronize() requires a single-bit signal
  --> src/broken.sk:15:29
   |
15 |     synced_data = synchronize(fast_data)
   |                               ^^^^^^^^^ `fast_data` is bit[8], not bit
   |
   = help: for multi-bit data, use an async FIFO or Gray code encoding
```

---

## Running Project: Dual-Clock UART

Up to now, the UART's transmitter, receiver, FIFOs, and control logic all share a single clock. In a real SoC, the UART peripheral connects to a system bus (AHB, APB, or AXI) that runs at the system clock frequency — say 100 MHz. But the UART serial interface might be clocked by a dedicated baud clock generated by a PLL, or the TX and RX might each have independent clock recovery circuits.

We will restructure the UART to have three clock domains:

- `'sys` — the system bus clock. The CPU writes TX data and reads RX data in this domain.
- `'tx_domain` — the transmitter clock. The TX shift register and baud generator run here.
- `'rx_domain` — the receiver clock. The RX sampler and bit reconstruction run here.

At each boundary, an **async FIFO** transfers data safely between domains.

### Async FIFO with Gray Code Pointers

A standard synchronous FIFO uses binary read and write pointers. If those pointers cross a clock domain boundary, multiple bits can change simultaneously (e.g., `0111` to `1000`), and a synchronizer sampling in the middle of that transition might capture `0000`, `1000`, or any other combination. The result is a corrupted pointer — the FIFO reads garbage or overflows.

**Gray code** solves this by guaranteeing that only one bit changes between consecutive values. A synchronizer sampling a Gray-coded pointer might see the old value or the new value, but never a corrupted intermediate — both the old and new values are valid pointers, and using the old value simply makes the FIFO appear one entry more full or empty than it actually is, which is conservative and safe.

Binary-to-Gray conversion is a single XOR: `gray = binary ^ (binary >> 1)`.

Create `src/async_fifo.sk`:

```
// async_fifo.sk — dual-clock FIFO with Gray code pointer synchronization
//
// Write side operates in the 'wr clock domain.
// Read side operates in the 'rd clock domain.
// Pointers are converted to Gray code before crossing domains.

entity AsyncFIFO<'wr, 'rd, const WIDTH: nat = 8, const DEPTH: nat = 16> {
    in  wr_clk:  clock<'wr>,
    in  rd_clk:  clock<'rd>,
    in  rst:     reset,

    // Write interface (all in 'wr domain)
    in  wr_en:   bit,
    in  wr_data: bit[WIDTH],
    out full:    bit,

    // Read interface (all in 'rd domain)
    in  rd_en:   bit,
    out rd_data: bit[WIDTH],
    out empty:   bit
}

impl AsyncFIFO {
    // Pointer width needs one extra bit for full/empty disambiguation.
    // A 16-deep FIFO uses 5-bit pointers (0-31), where the MSB
    // distinguishes "same position, empty" from "same position, full."
    signal ADDR_BITS: nat = clog2(DEPTH) + 1

    // --- Write domain signals ---
    signal wr_ptr:      nat[ADDR_BITS]       // binary write pointer
    signal wr_ptr_gray: bit[ADDR_BITS]       // Gray-coded write pointer

    // --- Read domain signals ---
    signal rd_ptr:      nat[ADDR_BITS]       // binary read pointer
    signal rd_ptr_gray: bit[ADDR_BITS]       // Gray-coded read pointer

    // --- Cross-domain synchronized pointers ---
    // These are Gray-coded pointers that have been synchronized
    // into the opposite clock domain using 2-flop synchronizers.
    #[cdc(cdc_type = gray, sync_stages = 2, from = 'wr, to = 'rd)]
    signal wr_ptr_gray_sync_rd: bit[ADDR_BITS]    // write ptr in read domain

    #[cdc(cdc_type = gray, sync_stages = 2, from = 'rd, to = 'wr)]
    signal rd_ptr_gray_sync_wr: bit[ADDR_BITS]    // read ptr in write domain

    // --- Synchronizer pipeline registers ---
    signal wr_gray_ff1: bit[ADDR_BITS]
    signal wr_gray_ff2: bit[ADDR_BITS]
    signal rd_gray_ff1: bit[ADDR_BITS]
    signal rd_gray_ff2: bit[ADDR_BITS]

    // --- Memory ---
    signal mem: bit[WIDTH][DEPTH]

    // ===================================================================
    // Write domain logic
    // ===================================================================
    on(wr_clk.rise) {
        if rst {
            wr_ptr = 0
        } else if wr_en & !full {
            mem[wr_ptr[clog2(DEPTH)-1:0]] = wr_data
            wr_ptr = wr_ptr + 1
        }
    }

    // Binary-to-Gray conversion for the write pointer.
    wr_ptr_gray = wr_ptr ^ (wr_ptr >> 1)

    // Synchronize the read pointer (Gray-coded) into the write domain.
    // This tells the write side how far the read side has progressed.
    on(wr_clk.rise) {
        if rst {
            rd_gray_ff1 = 0
            rd_gray_ff2 = 0
        } else {
            rd_gray_ff1 = rd_ptr_gray       // first flop (may go metastable)
            rd_gray_ff2 = rd_gray_ff1       // second flop (stable)
        }
    }
    rd_ptr_gray_sync_wr = rd_gray_ff2

    // Full detection: in Gray code, the FIFO is full when the write
    // pointer and the synchronized read pointer differ in the top two
    // bits but match in all remaining bits.
    full = (wr_ptr_gray[ADDR_BITS-1] != rd_ptr_gray_sync_wr[ADDR_BITS-1]) &
           (wr_ptr_gray[ADDR_BITS-2] != rd_ptr_gray_sync_wr[ADDR_BITS-2]) &
           (wr_ptr_gray[ADDR_BITS-3:0] == rd_ptr_gray_sync_wr[ADDR_BITS-3:0])

    // ===================================================================
    // Read domain logic
    // ===================================================================
    on(rd_clk.rise) {
        if rst {
            rd_ptr = 0
        } else if rd_en & !empty {
            rd_ptr = rd_ptr + 1
        }
    }

    // Binary-to-Gray conversion for the read pointer.
    rd_ptr_gray = rd_ptr ^ (rd_ptr >> 1)

    // Read data comes directly from memory at the read pointer address.
    rd_data = mem[rd_ptr[clog2(DEPTH)-1:0]]

    // Synchronize the write pointer (Gray-coded) into the read domain.
    // This tells the read side how much data is available.
    on(rd_clk.rise) {
        if rst {
            wr_gray_ff1 = 0
            wr_gray_ff2 = 0
        } else {
            wr_gray_ff1 = wr_ptr_gray       // first flop
            wr_gray_ff2 = wr_gray_ff1       // second flop
        }
    }
    wr_ptr_gray_sync_rd = wr_gray_ff2

    // Empty detection: the FIFO is empty when the read pointer's
    // Gray code matches the synchronized write pointer's Gray code.
    empty = (rd_ptr_gray == wr_ptr_gray_sync_rd)
}
```

### How the Async FIFO Works

**Pointer sizing.** For a FIFO of depth N, the pointers are `clog2(N) + 1` bits wide. The extra MSB serves as a wrap-around indicator. When the write pointer has gone around the buffer one more time than the read pointer, the FIFO is full. When they are equal, it is empty.

**Gray code conversion.** The expression `gray = binary ^ (binary >> 1)` converts a binary number to Gray code. For a 5-bit pointer counting from 0 to 31:

| Binary | Gray |
|--------|------|
| 00000  | 00000 |
| 00001  | 00001 |
| 00010  | 00011 |
| 00011  | 00010 |
| 00100  | 00110 |
| ...    | ...   |

Each consecutive Gray code value differs in exactly one bit. This property makes it safe to synchronize: the receiving flop sees either the old value (all bits consistent) or the new value (one bit changed), never a corrupted mixture.

**Full and empty detection.** In Gray code, the FIFO is empty when the synchronized write pointer equals the read pointer (both in Gray code). The FIFO is full when the top two bits differ but all lower bits match — this is the Gray code equivalent of "write pointer is exactly one full traversal ahead of the read pointer."

**The `#[cdc]` annotation.** The `#[cdc(cdc_type = gray, sync_stages = 2, from = 'wr, to = 'rd)]` annotation tells the compiler: "This signal is a Gray-coded value being synchronized from the `'wr` domain to the `'rd` domain through 2 synchronizer stages." The compiler verifies that:

1. The signal is indeed assigned in the target domain's `on` block
2. The source signal comes from a 2-stage flop chain
3. The Gray code property holds (source changes at most one bit per source clock cycle)

If any of these properties are violated, the build fails with a specific error.

### Dual-Clock UART Top Module

Now wire the async FIFOs into the UART. The system bus writes TX data into the write side of the TX FIFO (in the `'sys` domain), and the TX module reads from the read side (in the `'tx_domain`). The RX module writes received bytes into the write side of the RX FIFO (in the `'rx_domain`), and the system bus reads from the read side (in the `'sys` domain).

Create `src/uart_dual_clock.sk`:

```
// uart_dual_clock.sk — UART with separate clock domains for TX, RX, and system bus
//
// Three clock domains:
//   'sys        — system bus clock (CPU read/write)
//   'tx_domain  — transmitter clock (serial TX timing)
//   'rx_domain  — receiver clock (serial RX sampling)
//
// Async FIFOs bridge each boundary:
//   sys -> tx_domain:  TX data FIFO (CPU writes, TX reads)
//   rx_domain -> sys:  RX data FIFO (RX writes, CPU reads)

pub struct UartConfig {
    baud_div:    nat[16],
    parity_en:   bit,
    parity_odd:  bit,
    stop_bits_2: bit
}

pub struct UartStatus<'d> {
    tx_full:     bit<'d>,
    tx_empty:    bit<'d>,
    rx_full:     bit<'d>,
    rx_empty:    bit<'d>,
    rx_overrun:  bit<'d>
}

pub enum UartTxState: bit[2] {
    Idle  = 0,
    Start = 1,
    Data  = 2,
    Stop  = 3
}

pub enum UartRxState: bit[3] {
    Idle    = 0,
    Start   = 1,
    Data    = 2,
    Parity  = 3,
    Stop    = 4
}

entity UartDualClock<
    'sys, 'tx_domain, 'rx_domain,
    const DATA_WIDTH: nat = 8,
    const FIFO_DEPTH: nat = 16
> {
    // Clock and reset
    in  sys_clk: clock<'sys>,
    in  tx_clk:  clock<'tx_domain>,
    in  rx_clk:  clock<'rx_domain>,
    in  rst:     reset,

    // System bus interface (all in 'sys domain)
    in  config:     UartConfig,
    out status:     UartStatus<'sys>,
    in  tx_data:    bit[DATA_WIDTH],
    in  tx_wr_en:   bit,
    out rx_data:    bit[DATA_WIDTH],
    in  rx_rd_en:   bit,
    out rx_valid:   bit,

    // Physical pins
    out tx:  bit,          // TX serial output (driven from 'tx_domain)
    in  rx:  bit           // RX serial input (sampled in 'rx_domain)
}

impl UartDualClock {
    // =================================================================
    // TX Path: sys_clk -> AsyncFIFO -> tx_clk -> UartTx -> pin
    // =================================================================

    // Signals at the FIFO boundary
    signal tx_fifo_rd_data:  bit[DATA_WIDTH]
    signal tx_fifo_rd_en:    bit
    signal tx_fifo_empty:    bit
    signal tx_fifo_full:     bit

    // TX FIFO: write side in 'sys domain, read side in 'tx_domain
    let tx_fifo = AsyncFIFO<'sys, 'tx_domain, DATA_WIDTH, FIFO_DEPTH> {
        wr_clk:  sys_clk,
        rd_clk:  tx_clk,
        rst:     rst,
        wr_en:   tx_wr_en,
        wr_data: tx_data,
        full:    tx_fifo_full,
        rd_en:   tx_fifo_rd_en,
        rd_data: tx_fifo_rd_data,
        empty:   tx_fifo_empty
    }

    // TX engine (operates entirely in 'tx_domain)
    signal tx_state:     UartTxState
    signal tx_baud_cnt:  nat[16]
    signal tx_bit_idx:   nat[3]
    signal tx_shift:     bit[DATA_WIDTH]
    signal tx_baud_tick: bit

    tx_baud_tick = (tx_baud_cnt == 0)

    // Automatically read from FIFO when TX is idle and data is available
    tx_fifo_rd_en = (tx_state == UartTxState::Idle) & !tx_fifo_empty

    on(tx_clk.rise) {
        if rst {
            tx_state    = UartTxState::Idle
            tx_baud_cnt = 0
            tx_bit_idx  = 0
            tx_shift    = 0
        } else {
            match tx_state {
                UartTxState::Idle => {
                    if !tx_fifo_empty {
                        tx_shift    = tx_fifo_rd_data
                        tx_state    = UartTxState::Start
                        tx_baud_cnt = config.baud_div - 1
                        tx_bit_idx  = 0
                    }
                }

                UartTxState::Start => {
                    if tx_baud_tick {
                        tx_state    = UartTxState::Data
                        tx_baud_cnt = config.baud_div - 1
                    } else {
                        tx_baud_cnt = tx_baud_cnt - 1
                    }
                }

                UartTxState::Data => {
                    if tx_baud_tick {
                        tx_shift = tx_shift >> 1
                        if tx_bit_idx == (DATA_WIDTH - 1) {
                            tx_state    = UartTxState::Stop
                            tx_baud_cnt = config.baud_div - 1
                        } else {
                            tx_bit_idx  = tx_bit_idx + 1
                            tx_baud_cnt = config.baud_div - 1
                        }
                    } else {
                        tx_baud_cnt = tx_baud_cnt - 1
                    }
                }

                UartTxState::Stop => {
                    if tx_baud_tick {
                        tx_state = UartTxState::Idle
                    } else {
                        tx_baud_cnt = tx_baud_cnt - 1
                    }
                }
            }
        }
    }

    // TX output multiplexer
    tx = match tx_state {
        UartTxState::Idle  => 1,
        UartTxState::Start => 0,
        UartTxState::Data  => tx_shift[0],
        UartTxState::Stop  => 1
    }

    // =================================================================
    // RX Path: pin -> UartRx (rx_clk) -> AsyncFIFO -> sys_clk
    // =================================================================

    signal rx_fifo_wr_data:  bit[DATA_WIDTH]
    signal rx_fifo_wr_en:    bit
    signal rx_fifo_full_rx:  bit
    signal rx_fifo_empty:    bit

    // RX FIFO: write side in 'rx_domain, read side in 'sys domain
    let rx_fifo = AsyncFIFO<'rx_domain, 'sys, DATA_WIDTH, FIFO_DEPTH> {
        wr_clk:  rx_clk,
        rd_clk:  sys_clk,
        rst:     rst,
        wr_en:   rx_fifo_wr_en,
        wr_data: rx_fifo_wr_data,
        full:    rx_fifo_full_rx,
        rd_en:   rx_rd_en,
        rd_data: rx_data,
        empty:   rx_fifo_empty
    }

    // RX engine (operates entirely in 'rx_domain)
    signal rx_state:     UartRxState
    signal rx_baud_cnt:  nat[16]
    signal rx_bit_idx:   nat[3]
    signal rx_shift:     bit[DATA_WIDTH]
    signal rx_prev:      bit
    signal rx_done:      bit
    signal rx_overrun:   bit

    on(rx_clk.rise) {
        rx_prev   = rx
        rx_done   = 0

        if rst {
            rx_state    = UartRxState::Idle
            rx_baud_cnt = 0
            rx_bit_idx  = 0
            rx_shift    = 0
            rx_prev     = 1
            rx_overrun  = 0
        } else {
            match rx_state {
                UartRxState::Idle => {
                    rx_baud_cnt = 0
                    rx_bit_idx  = 0
                    if !rx & rx_prev {
                        rx_state    = UartRxState::Start
                        rx_baud_cnt = config.baud_div / 2
                    }
                }

                UartRxState::Start => {
                    if rx_baud_cnt == 0 {
                        if !rx {
                            rx_state    = UartRxState::Data
                            rx_baud_cnt = config.baud_div - 1
                        } else {
                            rx_state = UartRxState::Idle
                        }
                    } else {
                        rx_baud_cnt = rx_baud_cnt - 1
                    }
                }

                UartRxState::Data => {
                    if rx_baud_cnt == 0 {
                        rx_shift = (rx << (DATA_WIDTH - 1)) | (rx_shift >> 1)
                        if rx_bit_idx == (DATA_WIDTH - 1) {
                            rx_state    = UartRxState::Stop
                            rx_baud_cnt = config.baud_div - 1
                        } else {
                            rx_bit_idx  = rx_bit_idx + 1
                            rx_baud_cnt = config.baud_div - 1
                        }
                    } else {
                        rx_baud_cnt = rx_baud_cnt - 1
                    }
                }

                UartRxState::Parity => {
                    // Reserved for future parity support
                    rx_state    = UartRxState::Stop
                    rx_baud_cnt = config.baud_div - 1
                }

                UartRxState::Stop => {
                    if rx_baud_cnt == 0 {
                        rx_done  = 1
                        rx_state = UartRxState::Idle
                        if rx_fifo_full_rx {
                            rx_overrun = 1
                        }
                    } else {
                        rx_baud_cnt = rx_baud_cnt - 1
                    }
                }
            }
        }
    }

    // Write received bytes into the RX FIFO
    rx_fifo_wr_en   = rx_done & !rx_fifo_full_rx
    rx_fifo_wr_data = rx_shift

    // =================================================================
    // System domain: status and read interface
    // =================================================================

    // rx_valid pulses when a read is performed and data was available
    rx_valid = rx_rd_en & !rx_fifo_empty

    // Synchronize the rx_overrun flag from 'rx_domain to 'sys domain.
    // This is a single-bit flag, so a 2-flop synchronizer is sufficient.
    signal rx_overrun_sync: bit<'sys>
    signal overrun_ff1: bit
    signal overrun_ff2: bit

    on(sys_clk.rise) {
        if rst {
            overrun_ff1 = 0
            overrun_ff2 = 0
        } else {
            overrun_ff1 = rx_overrun
            overrun_ff2 = overrun_ff1
        }
    }
    rx_overrun_sync = overrun_ff2

    // Status port — all fields are in the 'sys domain.
    // tx_fifo_full and tx_fifo_empty come from the FIFO's write-side
    // and read-side respectively. For status reporting, we synchronize
    // as needed (the FIFO's full/empty outputs are already in the
    // appropriate domains by construction).
    status = UartStatus<'sys> {
        tx_full:    tx_fifo_full,
        tx_empty:   tx_fifo_empty,
        rx_full:    rx_fifo_full_rx,
        rx_empty:   rx_fifo_empty,
        rx_overrun: rx_overrun_sync
    }
}
```

### Architecture Overview

The data flow through the dual-clock UART follows two paths:

**TX path (system to serial):**

```
CPU --[sys_clk]--> TX FIFO write port
                      |
                  AsyncFIFO (Gray code pointers cross 'sys/'tx_domain boundary)
                      |
                   TX FIFO read port --[tx_clk]--> UartTx FSM --> TX pin
```

**RX path (serial to system):**

```
RX pin --[rx_clk]--> UartRx FSM --> RX FIFO write port
                                       |
                                   AsyncFIFO (Gray code pointers cross 'rx_domain/'sys boundary)
                                       |
                                    RX FIFO read port --[sys_clk]--> CPU
```

Every signal stays within its clock domain. The only points where data crosses a domain boundary are the async FIFOs and the single-bit `rx_overrun` synchronizer. The compiler verifies this automatically.

### What the Compiler Checks

When you build this design, the skalp compiler performs the following CDC verification:

1. **Domain assignment.** Every signal is tagged with the domain of the `on` block that assigns it. Signals assigned in `on(tx_clk.rise)` belong to `'tx_domain`. Signals assigned in `on(sys_clk.rise)` belong to `'sys`. Combinational signals inherit their domain from the signals they reference.

2. **Cross-domain reads.** Any read of a signal from domain A inside an `on` block of domain B is flagged as a CDC violation unless:
   - The read occurs inside a synchronizer entity (annotated with lifetime parameters)
   - The signal is routed through an `AsyncFIFO` or similar CDC primitive
   - The signal has a `#[cdc]` annotation that documents the synchronization strategy

3. **Annotation consistency.** The `#[cdc]` annotations are verified against the actual circuit structure. If you annotate a signal as `sync_stages = 2` but only have one flop in the chain, the compiler reports an error.

4. **Gray code integrity.** Signals annotated with `cdc_type = gray` are checked to ensure that only one bit changes per source clock cycle. If the source updates the pointer by more than 1 in a single cycle (e.g., writing two entries at once), the Gray code property is violated and the compiler rejects it.

---

> **Coming from SystemVerilog?**
>
> Clock domain crossing is where skalp provides its most dramatic improvement over SystemVerilog. Here is how the two approaches compare:
>
> | Aspect | SystemVerilog | skalp |
> |--------|---------------|-------|
> | Domain tracking | None — all signals are typeless with respect to clocks | Every clock carries a lifetime; every signal inherits a domain |
> | CDC detection | External tools (Spyglass, Meridian CDC) at $50K+/seat | Built into the compiler, runs on every build |
> | When bugs are found | Post-synthesis, during sign-off verification | At compile time, during development |
> | Synchronizer verification | Structural pattern matching by external tools | Type system guarantees through lifetime parameters |
> | Multi-bit CDC | Relies on designer discipline and code review | Compiler enforces Gray code or FIFO-based crossing |
> | False positives | CDC tools produce many, requiring manual waivers | Lifetime system is precise — no false positives |
> | Naming conventions | Teams adopt `_sync`, `_cdc_` prefixes by convention | Not needed — the type system tracks domains |
> | Cost | Verification tool licenses, CDC review meetings, waiver management | Zero — it is part of the language |
>
> In a typical SoC project, CDC verification consumes weeks of engineering time during the sign-off phase. Engineers write waiver files, debug false positives, and manually inspect every flagged crossing. With skalp, every CDC violation is caught the moment you write it, with a clear error message and a suggested fix. The entire category of "CDC sign-off" disappears from the project schedule.

---

## Build and Test

Compile the async FIFO standalone:

```bash
skalp build src/async_fifo.sk
```

Expected output:

```
   Compiling uart-tutorial v0.1.0
   Analyzing AsyncFIFO
   CDC check: 2 crossings verified (wr_ptr_gray: 'wr->'rd, rd_ptr_gray: 'rd->'wr)
       Built AsyncFIFO -> build/async_fifo.sv
```

Compile the full dual-clock UART:

```bash
skalp build src/uart_dual_clock.sk
```

Expected output:

```
   Compiling uart-tutorial v0.1.0
   Analyzing UartDualClock
   CDC check: 5 crossings verified
     - tx_fifo/wr_ptr_gray: 'sys -> 'tx_domain (gray, 2 stages)
     - tx_fifo/rd_ptr_gray: 'tx_domain -> 'sys (gray, 2 stages)
     - rx_fifo/wr_ptr_gray: 'rx_domain -> 'sys (gray, 2 stages)
     - rx_fifo/rd_ptr_gray: 'sys -> 'rx_domain (gray, 2 stages)
     - rx_overrun: 'rx_domain -> 'sys (2-flop, 2 stages)
       Built UartDualClock -> build/uart_dual_clock.sv
```

The CDC check line confirms that all domain crossings are properly synchronized. If you accidentally read a `'tx_domain` signal directly in the `on(sys_clk.rise)` block — even in a deeply nested conditional — the build fails immediately.

To simulate with multiple clocks:

```bash
skalp sim --entity UartDualClock \
    --clock sys_clk=100MHz \
    --clock tx_clk=50MHz \
    --clock rx_clk=50MHz \
    --cycles 50000 \
    --vcd build/uart_dual_clock.vcd
```

In the waveform viewer, verify:

1. Data written on `sys_clk` appears in the TX FIFO after the Gray code pointer synchronization latency (2-3 `tx_clk` cycles).
2. The TX module reads from the FIFO and serializes the data on `tx_clk`.
3. Received bytes written by the RX module on `rx_clk` appear in the RX FIFO read port after synchronization latency.
4. The `full` and `empty` flags update conservatively — they may lag by a few cycles but never produce a false "not full" or "not empty."

---

## Quick Reference

| Concept | Syntax | Example |
|---------|--------|---------|
| Clock domain lifetime | `clock<'name>` | `in sys_clk: clock<'sys>` |
| Signal with domain | `bit<'domain>` or `bit[N]<'domain>` | `in data_in: bit<'src>` |
| Entity with lifetimes | `entity Name<'a, 'b> { ... }` | `entity Synchronizer<'src, 'dst> { ... }` |
| Sequential block (domain) | `on(clk.rise)` | Signals assigned here belong to clk's domain |
| 2-flop synchronizer | `synchronize(signal)` | `synced = synchronize(async_flag)` |
| CDC annotation | `#[cdc(...)]` | `#[cdc(cdc_type = gray, sync_stages = 2, from = 'wr, to = 'rd)]` |
| Binary to Gray code | `gray = bin ^ (bin >> 1)` | `wr_ptr_gray = wr_ptr ^ (wr_ptr >> 1)` |
| Async FIFO instantiation | `let name = AsyncFIFO<...> { ... }` | `let tx_fifo = AsyncFIFO<'sys, 'tx_domain> { ... }` |
| Full detection (Gray) | Top 2 bits differ, rest match | See async FIFO implementation |
| Empty detection (Gray) | Gray pointers equal | `empty = (rd_ptr_gray == wr_ptr_gray_sync_rd)` |
| Compile-time log2 | `clog2(N)` | `clog2(DEPTH)` for address width |
| Struct with domain | `struct Name<'d> { ... }` | `pub struct UartStatus<'d> { tx_full: bit<'d> }` |
| CDC compile error | Automatic on cross-domain read | `error[E0401]: clock domain crossing without synchronization` |

---

## Next: Safety and Annotations

The UART is now a multi-clock, fully parameterized design with type-safe clock domain crossings. But hardware in safety-critical systems needs more than functional correctness — it needs protection against random hardware faults, visibility into internal state for debugging, and formal documentation of safety mechanisms.

In **[Chapter 9: Safety and Annotations](../09-safety-and-annotations/)**, you will learn how to:

- Add `#[safety_mechanism(type = tmr)]` annotations for triple modular redundancy
- Use `#[trace]` to expose internal signals for post-silicon debug
- Insert `#[breakpoint]` conditions that halt simulation when triggered
- Apply `#[retention]` to signals that must survive power-domain transitions
- Build a TMR voter entity and apply it to critical UART control signals

These annotations are metadata — they do not change the functional behavior of the design, but they instruct the compiler to generate additional protective logic, debug infrastructure, or verification checks. They are the skalp equivalent of synthesis directives, but type-checked and semantically verified.

Continue to [Chapter 9: Safety and Annotations](../09-safety-and-annotations/).
