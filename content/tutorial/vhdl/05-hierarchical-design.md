---
title: "Chapter 5: Hierarchical Design"
date: 2026-03-04
summary: "Multi-entity designs, direct entity instantiation, port maps, internal signals, and named associations — connect a sender and receiver through a bus with skalp."
tags: ["skalp", "vhdl", "tutorial", "hardware"]
weight: 5
ShowToc: true
---

## What This Chapter Teaches

Real hardware designs are never a single entity. VHDL's hierarchy system lets you define each module as a separate entity and wire them together inside a parent — exactly the way schematics work, but in text.

This chapter builds a minimal bus system: a sender, a receiver, and a top-level entity that wires them together through internal signals. By the end you will understand:

- How to define multiple entities in a single `.vhd` file (skalp supports this directly)
- How to split entities across files (and when you might prefer that)
- **Direct entity instantiation** — `entity work.sender` with no component declaration
- **Named association** in port maps — `clk => clk`
- **Internal signals** as glue between instances
- **Instance labels** — `u_sender:`, `u_receiver:` — and why they are required

The VHDL here uses direct entity instantiation exclusively — the modern, preferred style. Component declarations are covered briefly at the end for reference.

---

## The Design

The bus system has three entities: **`sender`** (drives data/valid onto the bus on trigger), **`receiver`** (captures data on valid, always ready), and **`bus_system`** (the top level that wires them together). All three live in a single file. Create `src/bus_system.vhd`:

```vhdl
library ieee;
use ieee.std_logic_1164.all;

-- Simple sender entity
entity sender is
    port (
        clk     : in  std_logic;
        rst     : in  std_logic;
        trigger : in  std_logic;
        tx_data : in  std_logic_vector(7 downto 0);
        data    : out std_logic_vector(7 downto 0);
        valid   : out std_logic;
        ready   : in  std_logic
    );
end entity sender;

architecture rtl of sender is
begin
    data  <= tx_data when trigger = '1' else (others => '0');
    valid <= trigger;
end architecture rtl;

-- Simple receiver entity
entity receiver is
    port (
        clk      : in  std_logic;
        rst      : in  std_logic;
        data     : in  std_logic_vector(7 downto 0);
        valid    : in  std_logic;
        ready    : out std_logic;
        rx_data  : out std_logic_vector(7 downto 0);
        rx_valid : out std_logic
    );
end entity receiver;

architecture rtl of receiver is
begin
    ready <= '1'; -- always ready

    process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                rx_data  <= (others => '0');
                rx_valid <= '0';
            elsif valid = '1' then
                rx_data  <= data;
                rx_valid <= '1';
            else
                rx_valid <= '0';
            end if;
        end if;
    end process;
end architecture rtl;

-- Top-level: connects sender to receiver via internal signals
entity bus_system is
    port (
        clk      : in  std_logic;
        rst      : in  std_logic;
        trigger  : in  std_logic;
        tx_data  : in  std_logic_vector(7 downto 0);
        rx_data  : out std_logic_vector(7 downto 0);
        rx_valid : out std_logic
    );
end entity bus_system;

architecture rtl of bus_system is
    signal bus_data  : std_logic_vector(7 downto 0);
    signal bus_valid : std_logic;
    signal bus_ready : std_logic;
begin
    u_sender: entity work.sender
        port map (
            clk     => clk,
            rst     => rst,
            trigger => trigger,
            tx_data => tx_data,
            data    => bus_data,
            valid   => bus_valid,
            ready   => bus_ready
        );

    u_receiver: entity work.receiver
        port map (
            clk      => clk,
            rst      => rst,
            data     => bus_data,
            valid    => bus_valid,
            ready    => bus_ready,
            rx_data  => rx_data,
            rx_valid => rx_valid
        );
end architecture rtl;
```

---

## How the Hierarchy Works

### Three Entities, One File

skalp processes all entities in a source file in declaration order. Because `sender` and `receiver` appear before `bus_system`, they are already analyzed by the time the top-level architecture references them. This "define before use" order is required — you cannot instantiate an entity that has not been declared yet in the same file.

Putting related entities in one file is convenient for small designs and tutorials. For larger projects, you would typically split each entity into its own file:

```
src/
  sender.vhd
  receiver.vhd
  bus_system.vhd
```

skalp compiles all `.vhd` files in `src/` and resolves cross-file references automatically. The `top` setting in `skalp.toml` tells the compiler which entity is the root of the hierarchy. Either approach — single file or multiple files — produces identical results.

### Direct Entity Instantiation

The key line is:

```vhdl
u_sender: entity work.sender
    port map ( ... );
```

This is **direct entity instantiation**. Let us break it apart:

- **`u_sender:`** — the instance label. Every instantiation in VHDL must have a unique label. Labels serve as identifiers in simulation (waveform viewers show `bus_system/u_sender/data`), in synthesis (for timing constraints and floorplanning), and in testbenches (for hierarchical signal access). The `u_` prefix is a common convention but not required.

- **`entity work.sender`** — this tells the compiler to use the entity named `sender` from the library named `work`. In VHDL, `work` is a built-in alias for the current project's library — it always refers to the entities you have compiled in your project. You never need to declare a library for `work`; it is always available.

- **`port map (...)`** — connects the instance's ports to signals in the enclosing architecture.

### Named Association

Inside the port map, each connection uses named association: `port_name => signal_name`. The left side is the port on the instantiated entity; the right side is the signal or port in the enclosing architecture.

VHDL also supports **positional association** (`port map (clk, rst, trigger, ...)`), where you list signals in port declaration order without names. Positional association is shorter, but it breaks silently if someone reorders the ports. Named association is self-documenting and robust — use it in all but the most trivial cases.

### Internal Signals as Glue

The three signals declared in `bus_system`'s architecture are the wires that connect the sender to the receiver:

```vhdl
signal bus_data  : std_logic_vector(7 downto 0);
signal bus_valid : std_logic;
signal bus_ready : std_logic;
```

These signals exist only inside `bus_system`. They are not visible from outside — the top-level ports expose `trigger`, `tx_data`, `rx_data`, and `rx_valid`, but the internal bus wiring is hidden. This is encapsulation: the parent decides how children are connected, and the outside world only sees the parent's ports.

The data flows through these signals: `u_sender` drives `bus_data` and `bus_valid`, `u_receiver` reads them. In the other direction, `u_receiver` drives `bus_ready`, which `u_sender` reads. The types must match exactly — if `sender` declares `data : out std_logic_vector(7 downto 0)` but `bus_data` is `std_logic_vector(3 downto 0)`, skalp reports a width mismatch at build time.

### Instance Labels

Every instance must have a label — this is not optional in VHDL. Labels must be unique within an architecture and serve as identifiers in simulation (waveform viewers show `bus_system/u_sender/data`), in synthesis constraints, and in testbenches for hierarchical signal access. Common conventions: `u_` or `i_` prefix for generic instances, `gen_` inside generate blocks (Chapter 9), or descriptive names like `tx_engine` when the role differs from the entity name.

### The Sender and Receiver

The **sender** is purely combinational — when `trigger` is high, it passes `tx_data` through to `data` and asserts `valid`. The `ready` input exists in the port list (the receiver drives it), but this simple sender ignores it. A real design would gate transmission on `ready`.

The **receiver** mixes combinational and sequential logic. The concurrent assignment `ready <= '1'` means it is always ready. The clocked process captures incoming data: when `valid` is asserted, `data` is latched into `rx_data` and `rx_valid` pulses high for one cycle. When `valid` drops, `rx_valid` clears — so `rx_valid` mirrors `valid` with one clock cycle of latency.

---

## Coming from skalp?

> If you have built hierarchical designs in skalp, the mapping is straightforward but the syntax is very different.
>
> In skalp, you instantiate a sub-entity with a let-binding and access its outputs with dot notation:
>
> ```
> let tx = sender { clk, rst, trigger, tx_data };
> let rx = receiver { clk, rst, data: tx.data, valid: tx.valid };
> // tx.data, rx.rx_data are directly accessible
> ```
>
> In VHDL, the same thing requires:
>
> 1. Declaring internal signals explicitly (`signal bus_data : ...`)
> 2. Writing a labeled instantiation with `entity work.sender`
> 3. Mapping every port by name in the `port map`
>
> The VHDL version is more verbose, but the underlying hardware is identical. Both describe the same netlist: two instances connected by wires.
>
> | skalp | VHDL | Notes |
> |-------|------|-------|
> | `let tx = sender { clk, rst, ... }` | `u_sender: entity work.sender port map (clk => clk, ...)` | Let-binding vs. labeled instantiation |
> | `tx.data` | Requires an explicit `signal bus_data` and port map entry | skalp auto-creates the wire |
> | Implicit wiring for same-name ports | Named association: `clk => clk` | skalp infers, VHDL is explicit |
> | No labels needed | Labels are mandatory (`u_sender:`) | VHDL labels appear in waveforms |
> | Type inference | Explicit types on all signals | VHDL requires full type declarations |

---

## Component Declarations (Legacy Style)

Before VHDL-93 introduced direct entity instantiation, the only way to instantiate an entity was through a **component declaration**. You will see this in older codebases — the pattern requires declaring a `component` block that duplicates the entity's port list, then instantiating with just the component name:

```vhdl
architecture rtl of bus_system is
    -- Component declaration (repeats the entity's port list)
    component sender is
        port (
            clk     : in  std_logic;
            rst     : in  std_logic;
            -- ... every port repeated ...
        );
    end component;
    signal bus_data : std_logic_vector(7 downto 0);
    -- ...
begin
    u_sender: sender  -- no "entity work." prefix
        port map ( clk => clk, ... );
end architecture rtl;
```

The problem is maintenance: if you change a port in the entity, you must update every component declaration that references it. Direct instantiation (`entity work.sender`) avoids this — the compiler reads ports directly from the entity declaration. skalp supports both styles, but prefer direct entity instantiation for all new code.

---

## Project Setup

Update your `skalp.toml` to set the top entity to `bus_system`:

```toml
[package]
name = "vhdl-tutorial"
version = "0.1.0"

[build]
lang = "vhdl"
top = "bus_system"
```

---

## Build and Simulate

### Building

```bash
skalp build
```

Expected output:

```
   Compiling vhdl-tutorial v0.1.0
   Analyzing sender
   Analyzing receiver
   Analyzing bus_system
       Built bus_system -> build/bus_system.vhd
```

Notice that skalp analyzes all three entities — it follows the hierarchy from `bus_system` through its instantiations.

---

## Testing the Design

Create `tests/bus_system_test.rs`:

```rust
use skalp_testing::Testbench;

#[tokio::test]
async fn test_bus_single_transfer() {
    let mut tb = Testbench::new("src/bus_system.vhd")
        .await
        .unwrap();
    tb.reset(2).await;

    // After reset, rx_data and rx_valid should be 0
    tb.expect("rx_data", 0u32).await;
    tb.expect("rx_valid", 0u8).await;

    // Load a byte and trigger a send
    tb.set("tx_data", 0xA5u32);
    tb.set("trigger", 1u8);
    tb.clock(1).await;

    // After one clock, the receiver should have captured the data
    // (sender is combinational, receiver latches on rising_edge)
    tb.expect("rx_data", 0xA5u32).await;
    tb.expect("rx_valid", 1u8).await;

    // Release trigger
    tb.set("trigger", 0u8);
    tb.clock(1).await;

    // rx_valid should drop, rx_data holds its last value
    tb.expect("rx_valid", 0u8).await;
}

#[tokio::test]
async fn test_bus_back_to_back_transfers() {
    let mut tb = Testbench::new("src/bus_system.vhd")
        .await
        .unwrap();
    tb.reset(2).await;

    let test_bytes: Vec<u32> = vec![0x00, 0xFF, 0x42, 0x81];

    for &byte in &test_bytes {
        tb.set("tx_data", byte);
        tb.set("trigger", 1u8);
        tb.clock(1).await;
        tb.expect("rx_data", byte).await;
        tb.expect("rx_valid", 1u8).await;

        tb.set("trigger", 0u8);
        tb.clock(1).await;
        tb.expect("rx_valid", 0u8).await;
    }
}

#[tokio::test]
async fn test_bus_no_trigger_no_data() {
    let mut tb = Testbench::new("src/bus_system.vhd")
        .await
        .unwrap();
    tb.reset(2).await;

    // Run 10 cycles without triggering — rx_valid should stay low
    tb.set("tx_data", 0xFFu32);
    tb.set("trigger", 0u8);
    for _ in 0..10 {
        tb.clock(1).await;
        tb.expect("rx_valid", 0u8).await;
    }
}
```

Run the tests:

```bash
cargo test
```

The three tests cover the basic transfer path, back-to-back transactions with edge-case values (0x00, 0xFF), and the idle case where no trigger means no spurious output. All three should pass.

**Exercise:** Add a test that holds `trigger` high for multiple consecutive cycles with changing `tx_data` values. Verify that `rx_data` tracks the input on every cycle and `rx_valid` stays high throughout.

---

## Quick Reference

| Concept | VHDL Syntax | Notes |
|---------|-------------|-------|
| Direct instantiation | `label: entity work.name port map (...)` | Preferred style, no component declaration |
| Component instantiation | `label: name port map (...)` | Legacy style, requires a component declaration |
| Component declaration | `component name is port (...); end component;` | Duplicates the entity interface |
| Named association | `port_name => signal_name` | Explicit, order-independent |
| Positional association | `signal1, signal2, ...` | Fragile, avoid in production code |
| Internal signal | `signal name : type;` | Declared between `is` and `begin` in architecture |
| Instance label | `u_name:` | Required, must be unique within an architecture |
| `work` library | `entity work.name` | Always refers to the current project |
| Multiple entities per file | Define in dependency order | skalp processes top-to-bottom |
| Multiple files | One entity per `.vhd` file in `src/` | skalp resolves cross-file references |
| Top entity | — | Set `top = "name"` in `skalp.toml` |

---

## Next: Testing VHDL with Rust

You have seen testbenches in passing throughout the first five chapters. Chapter 6 takes a deep dive into skalp's `Testbench` API: how to structure test files, write helper functions, generate waveform dumps from tests, test edge cases systematically, and organize a test suite for a multi-entity design.

Continue to [Chapter 6: Testing VHDL with Rust](../06-testing-with-rust/).
