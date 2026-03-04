---
title: "Chapter 9: Real-World Project"
date: 2026-03-04
summary: "Capstone: a parameterized SPI master with generics, generate statements, a five-state FSM, compile-time math, assertions, and a complete Rust test suite — all compiled and tested with skalp."
tags: ["skalp", "vhdl", "tutorial", "hardware"]
weight: 9
ShowToc: true
---

## What This Chapter Teaches

This is the capstone. Everything from the first eight chapters converges in a single, production-quality design: a parameterized SPI master controller.

The design is not a toy. It is a complete, configurable SPI Mode 0 (CPOL=0, CPHA=0) master that you could drop into an FPGA project and use to talk to flash chips, ADCs, DACs, or any SPI peripheral. It uses:

- **Generics** (Chapter 4) for clock frequency, SPI clock frequency, word size, and slave count
- **Compile-time math** with `IEEE.MATH_REAL` for automatic bit-width computation
- **An assertion** that validates parameter constraints before synthesis
- **Generate statements** for parameterized multi-slave chip-select logic
- **A five-state FSM** using the three-process pattern (Chapter 3)
- **A shift register** for full-duplex data transfer
- **A handshake protocol** (valid/ready) for the data interface
- **A complete Rust test suite** (Chapter 6) covering idle state, single-byte transfer, and loopback verification

By the end of this chapter you will have seen how all of these patterns compose in a real design, and you will have a template for building your own parameterized IP blocks with skalp.

---

## The SPI Master

SPI (Serial Peripheral Interface) is a synchronous serial protocol with four signals: SCLK (clock), MOSI (master out, slave in), MISO (master in, slave out), and CS_N (chip select, active low). The master drives SCLK and MOSI, reads MISO, and selects which slave to address by driving the appropriate CS_N line low.

This design implements SPI Mode 0: SCLK idles low, data is sampled on the rising edge and shifted on the falling edge. The master supports configurable word sizes, multiple slaves, and parameterized clock division.

Create `src/spi_master.vhd`:

```vhdl
library IEEE;
use IEEE.STD_LOGIC_1164.ALL;
use IEEE.NUMERIC_STD.ALL;
use IEEE.MATH_REAL.ALL;

entity SPI_MASTER is
    Generic (
        CLK_FREQ    : natural := 50e6;
        SCLK_FREQ   : natural := 5e6;
        WORD_SIZE   : natural := 8;
        SLAVE_COUNT : natural := 1
    );
    Port (
        CLK      : in  std_logic;
        RST      : in  std_logic;
        SCLK     : out std_logic;
        CS_N     : out std_logic_vector(SLAVE_COUNT-1 downto 0);
        MOSI     : out std_logic;
        MISO     : in  std_logic;
        DIN      : in  std_logic_vector(WORD_SIZE-1 downto 0);
        DIN_ADDR : in  std_logic_vector(
                       natural(ceil(log2(real(SLAVE_COUNT))))-1 downto 0);
        DIN_LAST : in  std_logic;
        DIN_VLD  : in  std_logic;
        DIN_RDY  : out std_logic;
        DOUT     : out std_logic_vector(WORD_SIZE-1 downto 0);
        DOUT_VLD : out std_logic
    );
end entity;
```

There is a lot packed into this entity declaration. Let us take it apart.

---

### Generic Parameters

```vhdl
Generic (
    CLK_FREQ    : natural := 50e6;
    SCLK_FREQ   : natural := 5e6;
    WORD_SIZE   : natural := 8;
    SLAVE_COUNT : natural := 1
);
```

Four generics control the entire design:

| Generic | Default | Purpose |
|---------|---------|---------|
| `CLK_FREQ` | 50 MHz | System clock frequency in Hz |
| `SCLK_FREQ` | 5 MHz | SPI clock frequency in Hz |
| `WORD_SIZE` | 8 | Bits per SPI transaction |
| `SLAVE_COUNT` | 1 | Number of chip-select lines |

All four use `natural` (non-negative integer). The defaults give you a 10:1 clock division with 8-bit words and a single slave -- the most common SPI configuration. To instantiate a 16-bit, 4-slave variant:

```vhdl
u_spi: entity work.SPI_MASTER
    generic map (
        CLK_FREQ    => 100e6,
        SCLK_FREQ   => 1e6,
        WORD_SIZE   => 16,
        SLAVE_COUNT => 4
    )
    port map ( ... );
```

The `natural` type (instead of `integer`) prevents accidental negative values at elaboration time. If someone passes `WORD_SIZE => -1`, the compiler rejects it immediately.

---

### Compile-Time Constants with `MATH_REAL`

The architecture opens with four computed constants:

```vhdl
architecture RTL of SPI_MASTER is
    constant DIVIDER_VALUE : natural := (CLK_FREQ/SCLK_FREQ)/2;
    constant WIDTH_CLK_CNT : natural := natural(ceil(log2(real(DIVIDER_VALUE))));
    constant WIDTH_ADDR    : natural := natural(ceil(log2(real(SLAVE_COUNT))));
    constant BIT_CNT_WIDTH : natural := natural(ceil(log2(real(WORD_SIZE))));
```

**`DIVIDER_VALUE`** is the number of system clock cycles per half-period of SCLK. With the defaults (50 MHz / 5 MHz / 2 = 5), the system counter counts from 0 to 4 and toggles SCLK, producing a 5 MHz SPI clock from a 50 MHz system clock.

**`ceil(log2(real(...)))`** computes the number of bits needed to represent a value. This is the VHDL equivalent of the `$clog2` function in Verilog. The expression chain works from the inside out:

1. `real(DIVIDER_VALUE)` -- cast the integer to a floating-point `real` (required by `log2`)
2. `log2(real(...))` -- compute the base-2 logarithm
3. `ceil(...)` -- round up to the next integer (you need 3 bits to represent 5 values, not 2.32)
4. `natural(...)` -- cast back to an integer for use in signal widths

The `IEEE.MATH_REAL` library provides `ceil`, `log2`, and `real`. These functions execute at compile time only -- they do not synthesize to hardware. They exist purely to compute constants for signal widths and loop bounds.

This pattern is how professional VHDL avoids hardcoded widths. Instead of declaring `signal sys_clk_cnt : unsigned(2 downto 0)` and hoping nobody changes `DIVIDER_VALUE`, you declare:

```vhdl
signal sys_clk_cnt : unsigned(WIDTH_CLK_CNT-1 downto 0);
```

Now the counter width automatically adjusts when the generic parameters change. Change `CLK_FREQ` to 100 MHz and the counter gets an extra bit. No manual recalculation needed.

---

### Compile-Time Assertion

```vhdl
begin
    ASSERT (DIVIDER_VALUE >= 5)
        REPORT "condition: SCLK_FREQ <= CLK_FREQ/10"
        SEVERITY ERROR;
```

This assertion fires at elaboration time if the clock ratio is too small. The SPI clock generator needs at least 5 system clocks per half-period to operate correctly (the FSM requires multiple clock edges per SPI clock transition for setup and hold timing). If someone instantiates the design with `CLK_FREQ => 10e6` and `SCLK_FREQ => 5e6`, the divider would be 1 -- far too small. The assertion catches this immediately instead of producing subtly broken timing.

VHDL assertions can have three severity levels: `NOTE`, `WARNING`, and `ERROR`. An `ERROR`-level assertion halts elaboration in skalp, preventing the design from compiling with invalid parameters. This is the hardware equivalent of a `static_assert` in C++ or a `compile_error!` in Rust.

---

### State Machine Type and Signal Declarations

```vhdl
    type state_t is (idle, first_edge, second_edge, transmit_end, transmit_gap);

    signal addr_reg        : unsigned(WIDTH_ADDR-1 downto 0);
    signal sys_clk_cnt     : unsigned(WIDTH_CLK_CNT-1 downto 0);
    signal sys_clk_cnt_max : std_logic;
    signal spi_clk         : std_logic;
    signal spi_clk_rst     : std_logic;
    signal din_last_reg_n  : std_logic;
    signal first_edge_en   : std_logic;
    signal second_edge_en  : std_logic;
    signal chip_select_n   : std_logic;
    signal load_data       : std_logic;
    signal miso_reg        : std_logic;
    signal shreg           : std_logic_vector(WORD_SIZE-1 downto 0);
    signal bit_cnt         : unsigned(BIT_CNT_WIDTH-1 downto 0);
    signal bit_cnt_max     : std_logic;
    signal rx_data_vld     : std_logic;
    signal master_ready    : std_logic;
    signal present_state   : state_t;
    signal next_state      : state_t;
```

The five FSM states trace a complete SPI byte transaction:

| State | Purpose |
|-------|---------|
| `idle` | Waiting for data. `DIN_RDY` is high, SCLK is low, CS_N is high (unless chaining). |
| `first_edge` | Rising edge of SCLK. MISO is sampled here (Mode 0). |
| `second_edge` | Falling edge of SCLK. The shift register advances. |
| `transmit_end` | All bits sent. SCLK is held low, DOUT_VLD pulses. |
| `transmit_gap` | Brief pause between words. Allows CS_N to deassert between transactions. |

Notice that signal widths use the computed constants: `unsigned(WIDTH_CLK_CNT-1 downto 0)` for the system clock counter, `unsigned(BIT_CNT_WIDTH-1 downto 0)` for the bit counter. These adjust automatically with the generics.

---

### System Clock Counter

```vhdl
    load_data <= master_ready and DIN_VLD;
    DIN_RDY   <= master_ready;

    sys_clk_cnt_max <= '1' when (to_integer(sys_clk_cnt) = DIVIDER_VALUE-1)
                           else '0';

    sys_clk_cnt_reg_p : process (CLK)
    begin
        if (rising_edge(CLK)) then
            if (RST = '1' or sys_clk_cnt_max = '1') then
                sys_clk_cnt <= (others => '0');
            else
                sys_clk_cnt <= sys_clk_cnt + 1;
            end if;
        end if;
    end process;
```

The system clock counter is a modulo-`DIVIDER_VALUE` counter. It counts from 0 to `DIVIDER_VALUE-1`, then resets. The `sys_clk_cnt_max` flag pulses high for one cycle at the terminal count, serving as a clock enable for the rest of the design.

The concurrent assignments `load_data <= master_ready and DIN_VLD` and `DIN_RDY <= master_ready` implement the handshake: data is loaded when the master is ready and the upstream source asserts valid. The `DIN_RDY` output reflects the master's readiness.

---

### SPI Clock Generator

```vhdl
    spi_clk_gen_p : process (CLK)
    begin
        if (rising_edge(CLK)) then
            if (RST = '1' or spi_clk_rst = '1') then
                spi_clk <= '0';
            elsif (sys_clk_cnt_max = '1') then
                spi_clk <= not spi_clk;
            end if;
        end if;
    end process;

    SCLK <= spi_clk;
```

Every time the system counter reaches its maximum, the SPI clock toggles. The `spi_clk_rst` signal, driven by the FSM output logic, forces SCLK low during idle, transmit_end, and transmit_gap states. This ensures SCLK idles low (Mode 0) and only toggles during active bit transfer.

---

### Bit Counter

```vhdl
    bit_cnt_max <= '1' when (bit_cnt = WORD_SIZE-1) else '0';

    bit_cnt_p : process (CLK)
    begin
        if (rising_edge(CLK)) then
            if (RST = '1' or spi_clk_rst = '1') then
                bit_cnt <= (others => '0');
            elsif (second_edge_en = '1') then
                bit_cnt <= bit_cnt + 1;
            end if;
        end if;
    end process;
```

The bit counter tracks how many bits have been transferred. It increments on each second (falling) edge of SCLK and resets when the FSM returns to idle. When it reaches `WORD_SIZE-1`, the `bit_cnt_max` flag tells the FSM that the last bit has been shifted.

---

### Multi-Slave Addressing with Generate Statements

```vhdl
    addr_reg_p : process (CLK)
    begin
        if (rising_edge(CLK)) then
            if (RST = '1') then
                addr_reg <= (others => '0');
            elsif (load_data = '1') then
                addr_reg <= unsigned(DIN_ADDR);
            end if;
        end if;
    end process;

    one_slave_g: if (SLAVE_COUNT = 1) generate
        CS_N(0) <= chip_select_n;
    end generate;

    more_slaves_g: if (SLAVE_COUNT > 1) generate
        cs_n_g : for i in 0 to SLAVE_COUNT-1 generate
            cs_n_p : process (addr_reg, chip_select_n)
            begin
                if (addr_reg = i) then
                    CS_N(i) <= chip_select_n;
                else
                    CS_N(i) <= '1';
                end if;
            end process;
        end generate;
    end generate;
```

This is where generate statements shine. The design supports any number of slaves, and the chip-select decoding logic adapts automatically.

**`if...generate`** selects between two implementations based on `SLAVE_COUNT`:

- When `SLAVE_COUNT = 1`, no address decoding is needed. The single `CS_N(0)` line directly follows `chip_select_n`. This avoids synthesizing a comparator that always returns true.

- When `SLAVE_COUNT > 1`, a **`for...generate`** loop creates one combinational process per slave. Each process compares `addr_reg` to its index `i`. The selected slave gets `chip_select_n` (which the FSM drives low during active transfer). All other slaves are held high (deselected).

The `addr_reg` register captures the slave address from `DIN_ADDR` at the start of each transaction. This ensures the chip-select lines remain stable throughout the transfer, even if `DIN_ADDR` changes.

Every `generate` block requires a label (`one_slave_g:`, `more_slaves_g:`, `cs_n_g:`). These labels appear in simulation hierarchies and are required by the VHDL language.

---

### Shift Register

```vhdl
    shreg_p : process (CLK)
    begin
        if (rising_edge(CLK)) then
            if (load_data = '1') then
                shreg <= DIN;
            elsif (second_edge_en = '1') then
                shreg <= shreg(WORD_SIZE-2 downto 0) & miso_reg;
            end if;
        end if;
    end process;

    DOUT <= shreg;
    MOSI <= shreg(WORD_SIZE-1);
```

The shift register handles full-duplex data transfer. When `load_data` is asserted, the transmit data from `DIN` is loaded. On each second edge (falling SCLK), the register shifts left by one bit: the MSB exits through `MOSI`, and the sampled `miso_reg` enters at the LSB.

After a complete word transfer, `shreg` contains the received data (read via `DOUT`), and all transmitted bits have been shifted out through `MOSI`. This single shift register simultaneously handles both directions -- a standard SPI technique.

The concatenation `shreg(WORD_SIZE-2 downto 0) & miso_reg` is the same shift pattern from Chapter 3, now parameterized with `WORD_SIZE`.

---

### MISO Sampling

```vhdl
    miso_reg_p : process (CLK)
    begin
        if (rising_edge(CLK)) then
            if (first_edge_en = '1') then
                miso_reg <= MISO;
            end if;
        end if;
    end process;
```

MISO is sampled on the first edge (rising SCLK) and held in `miso_reg` until the second edge shifts it into the shift register. This two-phase approach ensures clean sampling: the data is captured at the center of the SCLK high period and shifted during the SCLK low period.

---

### Three-Process FSM

The FSM is split across three processes. This is the canonical VHDL FSM structure that you will see in virtually every professional VHDL codebase.

#### Process 1: State Register

```vhdl
    fsm_present_state_p : process (CLK)
    begin
        if (rising_edge(CLK)) then
            if (RST = '1') then
                present_state <= idle;
            else
                present_state <= next_state;
            end if;
        end if;
    end process;
```

This is the only clocked process in the FSM. It does one thing: on each rising edge, copy `next_state` into `present_state`. On reset, return to `idle`. Nothing else. This separation makes the registered state obvious and keeps the timing path clean.

#### Process 2: Next-State Logic

```vhdl
    fsm_next_state_p : process (present_state, DIN_VLD, sys_clk_cnt_max, bit_cnt_max)
    begin
        case present_state is
            when idle =>
                if (DIN_VLD = '1') then
                    next_state <= first_edge;
                else
                    next_state <= idle;
                end if;
            when first_edge =>
                if (sys_clk_cnt_max = '1') then
                    next_state <= second_edge;
                else
                    next_state <= first_edge;
                end if;
            when second_edge =>
                if (sys_clk_cnt_max = '1') then
                    if (bit_cnt_max = '1') then
                        next_state <= transmit_end;
                    else
                        next_state <= first_edge;
                    end if;
                else
                    next_state <= second_edge;
                end if;
            when transmit_end =>
                if (sys_clk_cnt_max = '1') then
                    next_state <= transmit_gap;
                else
                    next_state <= transmit_end;
                end if;
            when transmit_gap =>
                if (sys_clk_cnt_max = '1') then
                    next_state <= idle;
                else
                    next_state <= transmit_gap;
                end if;
            when others =>
                next_state <= idle;
        end case;
    end process;
```

This is a purely combinational process. The sensitivity list includes every signal that the process reads: `present_state`, `DIN_VLD`, `sys_clk_cnt_max`, and `bit_cnt_max`. The process computes the next state based on the current state and these inputs.

The state transitions trace the SPI protocol:

1. **idle -> first_edge**: A valid data word arrives (`DIN_VLD = '1'`).
2. **first_edge -> second_edge**: The system counter reaches its maximum, marking one half-period of SCLK (rising edge).
3. **second_edge -> first_edge**: Another half-period passes (falling edge), and more bits remain.
4. **second_edge -> transmit_end**: The falling edge after the last bit.
5. **transmit_end -> transmit_gap**: One more half-period to cleanly deassert SCLK.
6. **transmit_gap -> idle**: A final gap for CS_N timing, then back to idle.

The `when others` clause is a safety net. Even though all five states are covered, VHDL requires it for completeness, and it provides a recovery path if the FSM enters an undefined state due to SEU (single-event upset) in radiation environments.

#### Process 3: Output Logic

```vhdl
    fsm_outputs_p : process (present_state, din_last_reg_n, sys_clk_cnt_max)
    begin
        case present_state is
            when idle =>
                master_ready   <= '1';
                chip_select_n  <= not din_last_reg_n;
                spi_clk_rst    <= '1';
                first_edge_en  <= '0';
                second_edge_en <= '0';
                rx_data_vld    <= '0';
            when first_edge =>
                master_ready   <= '0';
                chip_select_n  <= '0';
                spi_clk_rst    <= '0';
                first_edge_en  <= sys_clk_cnt_max;
                second_edge_en <= '0';
                rx_data_vld    <= '0';
            when second_edge =>
                master_ready   <= '0';
                chip_select_n  <= '0';
                spi_clk_rst    <= '0';
                first_edge_en  <= '0';
                second_edge_en <= sys_clk_cnt_max;
                rx_data_vld    <= '0';
            when transmit_end =>
                master_ready   <= '0';
                chip_select_n  <= '0';
                spi_clk_rst    <= '1';
                first_edge_en  <= '0';
                second_edge_en <= '0';
                rx_data_vld    <= sys_clk_cnt_max;
            when transmit_gap =>
                master_ready   <= '0';
                chip_select_n  <= not din_last_reg_n;
                spi_clk_rst    <= '1';
                first_edge_en  <= '0';
                second_edge_en <= '0';
                rx_data_vld    <= '0';
            when others =>
                master_ready   <= '0';
                chip_select_n  <= not din_last_reg_n;
                spi_clk_rst    <= '1';
                first_edge_en  <= '0';
                second_edge_en <= '0';
                rx_data_vld    <= '0';
        end case;
    end process;
end architecture;
```

The output logic is also purely combinational. Every output is explicitly assigned in every state branch -- no latches are inferred. This is critical: if you omit an assignment in any branch, VHDL infers a latch to hold the previous value, which is almost always a bug in FSM output logic.

Key outputs:

- **`master_ready`** -- high only in `idle`, allowing new data to be loaded.
- **`chip_select_n`** -- low during active transfer states, conditionally held low between chained words (controlled by `din_last_reg_n`).
- **`spi_clk_rst`** -- forces SCLK low during idle and transition states.
- **`first_edge_en` / `second_edge_en`** -- gated by `sys_clk_cnt_max`, enabling MISO sampling and shift operations at the correct SPI clock phase.
- **`rx_data_vld`** -- pulses at `transmit_end` when the system counter rolls over, signaling that `DOUT` contains valid received data.

---

## Design Patterns in This Code

The SPI master demonstrates several patterns that appear throughout professional VHDL:

### Three-Process FSM

The FSM is split into three distinct processes:

1. **State register** (clocked) -- holds the current state, handles reset
2. **Next-state logic** (combinational) -- computes the next state from inputs
3. **Output logic** (combinational) -- computes outputs from the current state

This decomposition has practical benefits. The state register is trivial to verify: it is a single flip-flop per state bit. The next-state logic can be analyzed independently for completeness (does every state handle every input?). The output logic can be checked for latch freedom by verifying that every output is assigned in every branch.

An alternative is the two-process FSM (merge next-state and output logic) or the one-process FSM (everything in a single clocked process with registered outputs). The three-process pattern is the most explicit and is the standard taught in VHDL textbooks.

### Generate-Based Parameterization

The `if...generate` and `for...generate` constructs create hardware at elaboration time based on generic parameters. This is fundamentally different from runtime conditional logic -- the generate block does not produce a multiplexer. It produces different circuits for different parameter values. When `SLAVE_COUNT = 1`, the multi-slave comparator logic does not exist in the synthesized design.

### Handshake Protocol (DIN_VLD / DIN_RDY)

The valid/ready handshake is the standard interface pattern for flow-controlled data transfer:

- The **source** asserts `DIN_VLD` when it has data available and holds `DIN` stable.
- The **sink** (the SPI master) asserts `DIN_RDY` when it can accept data.
- A transfer occurs when both `DIN_VLD` and `DIN_RDY` are high simultaneously.

This is the same handshake used by AXI-Stream, Avalon-ST, and most other streaming interfaces. The key rule: the source must not wait for `DIN_RDY` before asserting `DIN_VLD`, or the system can deadlock.

### Compile-Time Assertions

The `ASSERT` statement validates parameter relationships at elaboration time. This catches configuration errors before synthesis, not during simulation or (worse) on hardware. Every parameterized design should assert its constraints.

### Signal Naming Conventions

The design follows common VHDL naming conventions:

| Convention | Example | Meaning |
|------------|---------|---------|
| `_n` suffix | `chip_select_n`, `CS_N` | Active-low signal |
| `_reg` suffix | `addr_reg`, `miso_reg` | Registered (flip-flop) signal |
| `_cnt` suffix | `sys_clk_cnt`, `bit_cnt` | Counter |
| `_max` suffix | `sys_clk_cnt_max`, `bit_cnt_max` | Terminal count flag |
| `_en` suffix | `first_edge_en`, `second_edge_en` | Clock enable |
| `_vld` suffix | `DIN_VLD`, `rx_data_vld` | Data valid strobe |
| `_rdy` suffix | `DIN_RDY` | Ready to accept data |
| `_p` suffix | `sys_clk_cnt_reg_p` | Process label |
| `_g` suffix | `one_slave_g` | Generate label |

Consistent naming makes the design self-documenting. You can read the signal list and understand the architecture without reading the process bodies.

---

## Coming from skalp?

> If you have built designs in skalp's native language, the SPI master highlights the fundamental style differences between the two languages.
>
> **FSM structure.** The three-process FSM in VHDL is the most striking difference. In skalp, the same FSM would be a single `on(clk.rise)` block with `match state`:
>
> ```
> on(clk.rise) {
>     match state {
>         State::Idle => {
>             if din_vld {
>                 state = State::FirstEdge;
>                 shreg = din;
>             }
>         }
>         State::FirstEdge => {
>             if sys_clk_cnt_max {
>                 miso_reg = miso;
>                 state = State::SecondEdge;
>             }
>         }
>         // ...
>     }
> }
> ```
>
> skalp merges the state register, next-state logic, and output logic into a single imperative block. The compiler separates them during synthesis. VHDL requires you to perform this decomposition manually.
>
> **Generate statements vs. generic expressions.** VHDL needs `if...generate` and `for...generate` with labeled blocks to parameterize structure. skalp uses compile-time `if` and `for` directly in the entity body:
>
> ```
> for i in 0..SLAVE_COUNT {
>     cs_n[i] = if addr_reg == i { chip_select_n } else { 1 };
> }
> ```
>
> **Width computation.** VHDL requires `natural(ceil(log2(real(N))))` with `IEEE.MATH_REAL`. skalp has `clog2(N)` as a built-in.
>
> **Assertions.** VHDL `ASSERT` is a concurrent statement. skalp uses `static_assert!` with the same compile-time semantics.
>
> | VHDL | skalp | Notes |
> |------|-------|-------|
> | Three-process FSM | Single `on(clk.rise)` with `match` | skalp auto-decomposes |
> | `if...generate` / `for...generate` | Compile-time `if` / `for` | No labels needed in skalp |
> | `natural(ceil(log2(real(N))))` | `clog2(N)` | Built-in in skalp |
> | `ASSERT ... SEVERITY ERROR` | `static_assert!(cond, "msg")` | Same compile-time checking |
> | `signal shreg : slv(WORD_SIZE-1 downto 0)` | `signal shreg: bits[WORD_SIZE]` | skalp infers direction |
> | Sensitivity lists | Implicit | skalp tracks dependencies |

---

## Project Setup

Update your `skalp.toml` to set the top entity:

```toml
[package]
name = "vhdl-tutorial"
version = "0.1.0"

[build]
lang = "vhdl"
top = "SPI_MASTER"
```

Your project structure should now look like:

```
vhdl-tutorial/
  skalp.toml
  src/
    counter.vhd          # Chapter 1
    mux4.vhd             # Chapter 2
    timer.vhd            # Chapter 3
    gpio_controller.vhd  # Chapter 4
    bus_system.vhd       # Chapter 5
    spi_master.vhd       # Chapter 9 (this chapter)
  tests/
    counter_test.rs
    ...
    spi_master_test.rs   # Chapter 9 (this chapter)
```

Build the design:

```bash
skalp build
```

Expected output:

```
   Compiling vhdl-tutorial v0.1.0
   Analyzing SPI_MASTER
       Built SPI_MASTER -> build/spi_master.vhd
```

---

## Complete Test Suite

The SPI master is complex enough to warrant a thorough test suite. The tests below verify idle state behavior, single-byte transfers, and full loopback verification.

Create `tests/spi_master_test.rs`:

### Test 1: Idle State

```rust
use skalp_testing::Testbench;

const CLK_FREQ: u64 = 50_000_000;
const SCLK_FREQ: u64 = 5_000_000;
const DIVIDER: u64 = (CLK_FREQ / SCLK_FREQ) / 2;  // 5

#[tokio::test]
async fn test_spi_idle_state() {
    let mut tb = Testbench::new("src/spi_master.vhd", "SPI_MASTER").await.unwrap();
    tb.reset(2).await;
    tb.expect("DIN_RDY", 1u32).await;
    tb.expect("SCLK", 0u32).await;
    tb.expect("CS_N", 1u32).await;
}
```

This test verifies the post-reset state: the master is ready to accept data (`DIN_RDY = 1`), the SPI clock is idle low (`SCLK = 0`), and the chip select is deasserted (`CS_N = 1`). If any of these conditions fail, the FSM reset logic is broken.

The constants at the top mirror the design's default generics. `DIVIDER` is 5, meaning the system counter counts 0-4 before toggling SCLK. These constants help you reason about timing in the tests below.

### Test 2: Single-Byte Transfer

```rust
#[tokio::test]
async fn test_spi_single_byte_transfer() {
    let mut tb = Testbench::new("src/spi_master.vhd", "SPI_MASTER").await.unwrap();
    tb.reset(2).await;

    // Load data
    tb.set("DIN", 0xA5u32);
    tb.set("DIN_ADDR", 0u8);
    tb.set("DIN_LAST", 1u8);
    tb.set("DIN_VLD", 1u8);
    tb.clock(1).await;
    tb.set("DIN_VLD", 0u8);

    // Wait for transfer to complete
    for _ in 0..500 {
        tb.clock(1).await;
        if tb.get_u64("DOUT_VLD").await == 1 {
            break;
        }
    }
    tb.expect("DOUT_VLD", 1u32).await;
}
```

This test loads `0xA5` into the shift register and waits for the transfer to complete. The loop polls `DOUT_VLD` each cycle, breaking as soon as the master signals that the received word is valid. The 500-cycle limit is generous -- a real transfer with `DIVIDER = 5` and `WORD_SIZE = 8` takes about 80 system clocks -- but it prevents the test from hanging if the FSM gets stuck.

Note the handshake: `DIN_VLD` is asserted for one cycle while `DIN` holds the data. The master captures the data on the rising edge when both `DIN_VLD` and `DIN_RDY` are high, then deasserts `DIN_RDY` until the transfer completes. The test deasserts `DIN_VLD` after one cycle because the master only needs to see it for a single clock.

`DIN_LAST = 1` tells the master that this is the last word in the transaction, so CS_N should deassert after the transfer.

### Test 3: Loopback

```rust
#[tokio::test]
async fn test_spi_loopback() {
    let mut tb = Testbench::new("src/spi_master.vhd", "SPI_MASTER").await.unwrap();
    tb.reset(2).await;

    // Connect MOSI to MISO for loopback
    tb.set("DIN", 0x5Au32);
    tb.set("DIN_ADDR", 0u8);
    tb.set("DIN_LAST", 1u8);
    tb.set("DIN_VLD", 1u8);
    tb.clock(1).await;
    tb.set("DIN_VLD", 0u8);

    // Drive MISO from MOSI each cycle
    for _ in 0..500 {
        let mosi = tb.get_u64("MOSI").await;
        tb.set("MISO", mosi as u8);
        tb.clock(1).await;
        if tb.get_u64("DOUT_VLD").await == 1 {
            break;
        }
    }

    tb.expect("DOUT_VLD", 1u32).await;
    tb.expect("DOUT", 0x5Au32).await;
}
```

The loopback test is the most important test in the suite. It connects `MOSI` back to `MISO` in software: each cycle, the test reads the current value of `MOSI` and drives it onto `MISO`. If the shift register, clock generation, and sampling logic all work correctly, the transmitted byte (`0x5A`) should arrive unchanged in `DOUT`.

This test catches a wide class of bugs:

- **Off-by-one in bit counting**: if the counter is wrong, the received word will be shifted or truncated
- **Wrong sampling edge**: if MISO is sampled on the falling edge instead of the rising edge, the data will be corrupted
- **Shift direction errors**: if the shift register shifts right instead of left, the bit order will be reversed
- **Clock generation bugs**: if SCLK does not toggle correctly, some bits will be skipped or doubled

If the loopback test passes with the correct data, the core SPI data path is working.

### Running the Tests

```bash
cargo test --test spi_master_test
```

Expected output:

```
running 3 tests
test test_spi_idle_state ... ok
test test_spi_single_byte_transfer ... ok
test test_spi_loopback ... ok

test result: ok. 3 passed; 0 finished in 0.25s
```

To generate waveforms for debugging:

```bash
skalp sim --entity SPI_MASTER --cycles 200 --vcd build/spi_master.vcd
```

Open the VCD file in the skalp VS Code extension to see SCLK toggling, MOSI shifting out data, CS_N framing the transaction, and the FSM state transitions.

**Exercise:** Add a test that sends two consecutive words without deasserting CS_N between them. Set `DIN_LAST = 0` for the first word and `DIN_LAST = 1` for the second. Verify that CS_N stays low throughout both transfers and only deasserts after the second word completes.

---

## Tutorial Complete

You have reached the end of the VHDL tutorial. Over nine chapters, you have gone from compiling a simple counter to building and testing a production-quality SPI master -- all with skalp.

Here is what each chapter covered:

| Chapter | Topic | Key Concepts |
|---------|-------|--------------|
| 1 | [Getting Started](../01-getting-started/) | Entity/architecture, `rising_edge`, `skalp build`, basic simulation |
| 2 | [Combinational Logic](../02-combinational-logic/) | `process(all)`, `case/when`, `when...else`, `with...select` |
| 3 | [Clocked Processes and FSMs](../03-processes-and-fsms/) | Enumerated types, `case state is`, type casting, named processes |
| 4 | [Generics, Records, and Arrays](../04-generics-and-types/) | Generic parameters, array types, register banks, edge detection |
| 5 | [Hierarchical Design](../05-hierarchical-design/) | Multi-entity designs, direct instantiation, port maps, internal signals |
| 6 | [Testing VHDL with Rust](../06-testing-with-rust/) | `Testbench` API: `set`, `clock`, `expect`, `get_u64`, waveform dumps |
| 7 | [skalp Integration](../07-skalp-integration/) | `-- skalp:` pragmas, formal verification, mixed skalp+VHDL designs |
| 8 | [VHDL-2019 Features](../08-vhdl-2019/) | Interfaces, views, generic types -- features most free tools lack |
| 9 | [Real-World Project](../09-real-world-project/) | Parameterized SPI master, generate statements, three-process FSM, complete test suite |

The core workflow is always the same:

1. Write VHDL in `src/`
2. `skalp build` to compile
3. Write Rust tests in `tests/`
4. `cargo test` to verify
5. `skalp sim --vcd` to debug with waveforms

This is the workflow that skalp was designed to enable: write your VHDL, test it with Rust, iterate fast.

---

## Where to Go Next

**Learn the skalp language.** If you have been working in VHDL and want to try skalp's native language, the [skalp Tutorial](/tutorial/skalp/) builds a complete UART peripheral from scratch. It covers the same hardware concepts (FSMs, generics, hierarchies) but with skalp's Rust-inspired syntax, type inference, and built-in safety features.

**Explore the skalp project.** The [skalp project page](/projects/skalp/) has architecture details, a feature comparison with other tools, and links to the compiler internals.

**Browse the source.** The [skalp GitHub repository](https://github.com/girivs82/skalp) has additional examples in `examples/vhdl/`, including the SPI master from this chapter, along with issue tracking and release notes.

---

## Quick Reference

This table summarizes the VHDL constructs introduced or reinforced in this chapter:

| Concept | VHDL Syntax | Notes |
|---------|-------------|-------|
| Generic parameter | `generic (NAME : natural := value)` | Compile-time constant, set at instantiation |
| Compile-time width | `natural(ceil(log2(real(N))))` | Requires `IEEE.MATH_REAL` |
| Assertion | `ASSERT cond REPORT "msg" SEVERITY ERROR` | Fires at elaboration time |
| Enumerated type | `type state_t is (idle, first_edge, ...)` | Symbolic FSM states |
| Three-process FSM | State register + next-state logic + output logic | Canonical VHDL FSM structure |
| `if...generate` | `label: if (cond) generate ... end generate;` | Conditional structural code |
| `for...generate` | `label: for i in range generate ... end generate;` | Replicated structural code |
| Shift register | `shreg <= shreg(N-2 downto 0) & new_bit` | Concatenation-based left shift |
| Terminal count | `flag <= '1' when (cnt = MAX) else '0'` | Concurrent comparison |
| Handshake | `VLD` + `RDY`, transfer when both high | Standard flow control |
| `natural` type | `CLK_FREQ : natural := 50e6` | Non-negative integer, prevents negative generics |
| `to_integer` | `to_integer(unsigned_signal)` | Convert `unsigned` to integer for comparison |
| Active-low naming | `CS_N`, `chip_select_n` | `_n` suffix convention |
| Process label | `fsm_present_state_p : process (CLK)` | Appears in waveform hierarchy |
| Generate label | `one_slave_g: if ... generate` | Required by VHDL, appears in simulation |
