# blinky_d0 — D0 (C906) LED Blink for Sipeed M1s Dock

Toggles the M1s Dock's red LED (GPIO 8, active-low) from the D0 C906 core,
demonstrating the minimum HAL usage: `glb.split()` for clock control and pin
allocation, then `into_push_pull_output()` followed by a spin-delay blink loop.

---

## Hardware

| Signal | GPIO | Active level | Notes                                          |
|--------|------|--------------|------------------------------------------------|
| LED1   | 8    | LOW          | Anode to 3.3 V via 1 kΩ; cathode to GPIO 8    |

Source: `apps/blinky_d0/src/main.rs` doc comment, confirmed against Sipeed SDK
`hal_board.c` GPIO table for the M1s Dock board variant.

---

## Prerequisites

### Rust toolchain

The crate uses Rust 2021 edition with no nightly-only features; a recent stable
toolchain works.

```sh
rustup target add riscv64imac-unknown-none-elf
```

The D0 C906 is a 64-bit RISC-V core (RV64IMAC), so the target triple is
`riscv64imac-unknown-none-elf`. This is confirmed by the root `.cargo/config.toml`
(`[target.riscv64imac-unknown-none-elf]`) and by `memory.x` using 64-bit XIP
addresses (`ORIGIN = 0x58002000`).

### Linker

The workspace `.cargo/config.toml` passes `-Tmemory.x -Tlink.x`. `link.x` is
provided by `riscv-rt`; `memory.x` is copied to the build output directory by
`build.rs`.

### Binary extraction tool

To flash you need `riscv64-unknown-elf-objcopy` (from any RISC-V GCC toolchain)
or `llvm-objcopy` to strip the ELF into a raw binary.

### Flashing tool

[Bouffalo Lab Dev Cube](https://dev.bouffalolab.com/download) (GUI) or the
`bflb-iot-tool` Python CLI:

```sh
pip install bflb-iot-tool
```

Both are documented in `bl808-pac/bl_mcu_sdk_bl808/README.md`.

---

## Build

```sh
cd apps/blinky_d0
cargo build --release
```

The ELF lands at:

```
/mnt/ramdisk/bl808-hal/target/riscv64imac-unknown-none-elf/release/blinky_d0
```

(The `target-dir` is overridden to `/mnt/ramdisk/bl808-hal/target` in the root
`.cargo/config.toml`; on a standard checkout it would be `../../target/...`.)

### Produce a flashable binary

```sh
riscv64-unknown-elf-objcopy \
    -O binary \
    /mnt/ramdisk/bl808-hal/target/riscv64imac-unknown-none-elf/release/blinky_d0 \
    blinky_d0.bin
```

---

## Flash Partition Layout (M1s Dock, 16 MiB)

Source: `bl808-pac/M1s_BL808_SDK/partition/partition_cfg_16M_m1sdock.toml`

| Partition | Type | Flash offset | Size       | Notes                                  |
|-----------|------|--------------|------------|----------------------------------------|
| Boot2     | 16   | `0x00000000` | 56 KiB (`0xE000`)   | Second-stage bootloader       |
| (PT table)|  —   | `0x0000E000` | 4 KiB × 2 | Partition table (primary + backup)     |
| FW (M0)   | 0    | `0x00010000` | 960 KiB (`0xF0000`) | M0 E907 application firmware  |
| D0FW      | 2    | `0x00100000` | 2 MiB (`0x200000`)  | **D0 C906 application firmware** |
| media     | 5    | `0x00300000` | 12 MiB (`0xC00000`) | Media / filesystem area       |
| factory   | 8    | `0x00910000` | 0          | Factory / DTS data                     |
| unused    | 11   | `0x00F00000` | 1 MiB (`0x100000`)  | Reserved                       |

`blinky_d0.bin` belongs in the **D0FW** partition at flash offset `0x100000`.

Note: the `memory.x` XIP origin (`0x58002000`) reflects the D0's XIP window
base (`0x58000000`) plus an 8 KiB header offset that Boot2 prepends to the
image. The flashing tools handle the header automatically when the `header = 1`
partition flag is set (as it is for D0FW in the partition table).

---

## Entering Download Mode

1. Connect the M1s Dock's **UART** USB-C port to your host (the port labelled
   `UART` or connected to the CH340 / CP2102 bridge, **not** the OTG port).
2. Hold the **BOOT** button on the M1s Dock.
3. Press and release **RESET** (or power-cycle) while keeping BOOT held.
4. Release BOOT. The chip is now in UART download mode.

---

## Flashing with Bouffalo Lab Dev Cube (GUI)

1. Open Dev Cube, select chip **BL808**, port **MCU**.
2. In the **Image** tab:
   - Set **Partition Table** to
     `bl808-pac/M1s_BL808_SDK/partition/partition_cfg_16M_m1sdock.toml`.
   - Set **D0 Firmware** to `blinky_d0.bin`.
3. Select the correct serial port and baud rate (2 000 000 is typical).
4. Click **Create & Download**.

## Flashing with bflb-iot-tool (CLI)

```sh
bflb-iot-tool \
  --chipname BL808 \
  --port /dev/ttyUSB0 \
  --baudrate 2000000 \
  --pt bl808-pac/M1s_BL808_SDK/partition/partition_cfg_16M_m1sdock.toml \
  --d0fw blinky_d0.bin
```

Adjust `--port` to match your system (`/dev/ttyUSB1` on Linux with a CH340 on
some machines; `COM3` etc. on Windows).

---

## Boot Flow

1. The on-chip ROM loads **Boot2** from flash offset `0x000000`.
2. Boot2 reads the partition table and loads the **M0 FW** (`0x010000`) to
   bring up the wireless core and clock tree.
3. Boot2 then loads **D0FW** (`0x100000`) and releases the C906 reset. The
   C906 begins executing `blinky_d0` from the XIP window at `0x58002000`
   (= `0x58000000` + image header size).
4. The HAL's `glb.split()` captures the already-configured clock tree;
   GPIO 8 is configured as push-pull output and the blink loop starts.

---

## Expected Behaviour

After flashing and resetting normally (without BOOT held), the red LED on the
M1s Dock blinks approximately once per second. At the default 400 MHz D0 clock,
each `spin_delay(1_000_000)` iteration is roughly 1–4 ms, so total period is
a few seconds; adjust the constant for a different cadence.
