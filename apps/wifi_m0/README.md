# wifi_m0 — M0 (E907) WiFi Coexistence + IPC Demo for Sipeed M1s Dock

Demonstrates peripheral initialisation and inter-processor communication (IPC)
on the BL808 M0 E907 core. The app enables BT/WiFi coexistence arbitration,
waits for the MAC firmware to signal readiness over an IPC channel, then runs a
continuous background-scan loop, signalling the D0 core via IPC after each
scan cycle.

This is a **protocol skeleton**, not a complete WiFi stack. A production WiFi
connection requires the proprietary Bouffalo Lab MAC firmware blob running as
the M0 application; that blob is not included in this repository. The scan
loop here demonstrates the IPC API surface and async executor pattern that
a real driver would build on top of.

---

## Architecture

```
  M0 (E907, this firmware)          D0 (C906)
  ┌──────────────────────┐          ┌─────────────────────┐
  │ 1. enable_coex()     │          │                     │
  │ 2. wait MAC ready    │◄─ IPC ──│ MAC blob (or stub)  │
  │    (CH 0, D0→M0)     │          │  signals CH_MAC_READY│
  │ 3. trigger scan      │── IPC ──►│ CH_SCAN_REQ         │
  │    (CH 0, M0→D0)     │          │                     │
  │ 4. wait scan done    │◄─ IPC ──│ CH_SCAN_DONE (CH 1) │
  │ 5. repeat every ~5 s │          │                     │
  └──────────────────────┘          └─────────────────────┘
```

The async executor is a bare-metal spin-poll loop using `async-executor` and
`futures-lite` in `no_std` mode. `futures_lite::future::block_on` cannot be
used in firmware (it requires `std`); instead the top-level future is pinned on
the stack and polled directly with a no-op waker.

### IPC Channel Assignments

| Constant       | Direction | Bit mask | Purpose                        |
|----------------|-----------|----------|--------------------------------|
| `CH_MAC_READY` | D0 → M0   | `1 << 0` | MAC init/self-test complete    |
| `CH_SCAN_REQ`  | M0 → D0   | `1 << 0` | Request passive background scan|
| `CH_SCAN_DONE` | D0 → M0   | `1 << 1` | Scan cycle finished            |

### HAL API Used

| Call                          | Register touched              |
|-------------------------------|-------------------------------|
| `WifiDriver::new(dp.wifi)`    | `bl808_pac::Wifi`             |
| `WifiDriver::enable_coex()`   | `coex_control_0` bit 0        |
| `WifiDriver::get_coex_state()`| `coex_state`                  |
| `Ipc::new(dp.ipc)`            | `bl808_pac::Ipc`              |
| `Ipc::unmask_m0(mask)`        | `CPU0_IPC_IUSR`               |
| `Ipc::pending_m0()`           | `CPU0_IPC_IRSRR`              |
| `Ipc::clear_m0(mask)`         | `CPU0_IPC_ICR` (W1C)          |
| `Ipc::trigger_d0(mask)`       | `CPU1_IPC_ISWR`               |

---

## Prerequisites

### Rust toolchain

Rust 2021 edition; recent stable works.

```sh
rustup target add riscv32imac-unknown-none-elf
```

The M0 E907 is a 32-bit RISC-V core (RV32IMAC), so the target triple is
`riscv32imac-unknown-none-elf`. Confirmed by the root `.cargo/config.toml`
(`[target.riscv32imac-unknown-none-elf]`) and `memory.x` using 32-bit RAM
addresses (`ORIGIN = 0x42030000`).

### Linker

The workspace `.cargo/config.toml` passes `-Tmemory.x -Tlink.x`. `link.x` is
supplied by `riscv-rt`; `memory.x` is copied to the build output directory by
`build.rs`.

### Binary extraction tool

`riscv32-unknown-elf-objcopy` (or `llvm-objcopy`) to produce a raw binary from
the ELF.

### Flashing tool

[Bouffalo Lab Dev Cube](https://dev.bouffalolab.com/download) (GUI) or
`bflb-iot-tool`:

```sh
pip install bflb-iot-tool
```

---

## Memory Map (from `memory.x`)

| Region | Origin       | Length | Description                          |
|--------|--------------|--------|--------------------------------------|
| FLASH  | `0x58000000` | 16 MiB | XIP flash window for the M0 core     |
| RAM    | `0x42030000` | 112 KiB| Wireless RAM (WRAM), dedicated to M0 |

The M0 XIP window starts at `0x58000000` (no header offset — Boot2 places M0
firmware without the extra 8 KiB padding used for the D0 image).

---

## Build

```sh
cd apps/wifi_m0
cargo build --release
```

ELF output:

```
/mnt/ramdisk/bl808-hal/target/riscv32imac-unknown-none-elf/release/wifi_m0
```

### Produce a flashable binary

```sh
riscv32-unknown-elf-objcopy \
    -O binary \
    /mnt/ramdisk/bl808-hal/target/riscv32imac-unknown-none-elf/release/wifi_m0 \
    wifi_m0.bin
```

---

## Flash Partition Layout (M1s Dock, 16 MiB)

Source: `bl808-pac/M1s_BL808_SDK/partition/partition_cfg_16M_m1sdock.toml`

| Partition | Type | Flash offset | Size        | Notes                                   |
|-----------|------|--------------|-------------|------------------------------------------|
| Boot2     | 16   | `0x00000000` | 56 KiB (`0xE000`)    | Second-stage bootloader        |
| (PT table)|  —   | `0x0000E000` | 4 KiB × 2  | Partition table (primary + backup)       |
| **FW (M0)**| 0   | `0x00010000` | 960 KiB (`0xF0000`)  | **M0 E907 application firmware** |
| D0FW      | 2    | `0x00100000` | 2 MiB (`0x200000`)   | D0 C906 application firmware   |
| media     | 5    | `0x00300000` | 12 MiB (`0xC00000`)  | Media / filesystem area        |
| factory   | 8    | `0x00910000` | 0           | Factory / DTS data                       |
| unused    | 11   | `0x00F00000` | 1 MiB (`0x100000`)   | Reserved                        |

`wifi_m0.bin` belongs in the **FW (M0)** partition at flash offset `0x010000`.

---

## Entering Download Mode

1. Connect the M1s Dock **UART** USB-C port to your host (the CH340/CP2102
   bridge, **not** the OTG port).
2. Hold the **BOOT** button.
3. Press and release **RESET** (or power-cycle) while BOOT is held.
4. Release BOOT. The chip is now in UART download mode.

---

## Flashing with Bouffalo Lab Dev Cube (GUI)

1. Open Dev Cube, select chip **BL808**, port **MCU**.
2. In the **Image** tab:
   - Set **Partition Table** to
     `bl808-pac/M1s_BL808_SDK/partition/partition_cfg_16M_m1sdock.toml`.
   - Set **M0 Firmware** to `wifi_m0.bin`.
3. Select the correct serial port and baud rate (2 000 000 typical).
4. Click **Create & Download**.

## Flashing with bflb-iot-tool (CLI)

```sh
bflb-iot-tool \
  --chipname BL808 \
  --port /dev/ttyUSB0 \
  --baudrate 2000000 \
  --pt bl808-pac/M1s_BL808_SDK/partition/partition_cfg_16M_m1sdock.toml \
  --firmware wifi_m0.bin
```

Adjust `--port` for your system. The `--firmware` flag targets the M0 FW
partition (`0x010000`).

---

## Boot Flow

1. The on-chip ROM loads **Boot2** from `0x000000`.
2. Boot2 reads the partition table (primary at `0xE000`, backup at `0xF000`).
3. Boot2 loads the **M0 FW** from `0x010000` into WRAM and releases the E907
   reset. The E907 begins executing `wifi_m0` via XIP from `0x58000000`.
4. The M0 application initialises `WifiDriver` and `Ipc`, then spins in the
   async poll loop.
5. Boot2 subsequently loads and releases **D0FW** (`0x100000`). The D0 is
   expected to run the MAC firmware blob or a stub that responds to IPC
   channels.

The M0 boots **before** the D0 in this flow. The `wait_mac_ready` phase in
`wifi_task` handles the window where the D0 has not yet completed its MAC init.

---

## IPC / Shared-Memory Convention

The IPC peripheral has independent 16-bit trigger registers for each core:

- **M0 receives** from D0 via `CPU0_IPC_IRSRR` (pending) and `CPU0_IPC_ICR`
  (W1C clear). Channels are unmasked by writing `CPU0_IPC_IUSR`.
- **M0 sends** to D0 by writing `CPU1_IPC_ISWR`.

Actual scan results (BSS entries) are exchanged through a shared SRAM region
whose base address and layout must be agreed at link time between the M0 and
D0 firmware images. This application leaves that region as a comment
placeholder; a real driver would define a `#[link_section]` struct at a fixed
address visible from both linker scripts.

---

## Limitations

- A live WiFi scan requires the proprietary Bouffalo Lab MAC firmware blob
  flashed as the M0 application (or embedded alongside it). This demo app is a
  structural skeleton demonstrating the IPC and coex API; without the MAC blob
  the `wait_mac_ready` loop will spin indefinitely.
- The async runtime is a no-op-waker spin loop. It is appropriate for
  single-core bare-metal polling but wastes power; a production driver would
  use PLIC interrupt delivery to avoid spinning.
- `spin_ms` is calibrated for a 160 MHz E907 clock (`LOOPS_PER_MS = 40_000`).
  If Boot2 leaves the M0 at a different frequency, the 5 s inter-scan delay
  will be proportionally off.
