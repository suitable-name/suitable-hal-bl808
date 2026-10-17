#!/usr/bin/env python3
"""
Generate per-peripheral HAL examples for BL808 M0, D0, and LP cores.
Each example is a self-contained, no_std, no_main Rust crate under examples/
demonstrating type-safe HAL driver usage and embedded-hal trait integration.
"""

import os
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
EXAMPLES_DIR = os.path.join(REPO_ROOT, "examples")

MEMORY_X_M0 = """MEMORY
{
    FLASH : ORIGIN = 0x58000000, LENGTH = 4M
    RAM   : ORIGIN = 0x22020000, LENGTH = 64K
}
REGION_ALIAS("REGION_TEXT", FLASH);
REGION_ALIAS("REGION_RODATA", FLASH);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);
REGION_ALIAS("REGION_HEAP", RAM);
REGION_ALIAS("REGION_STACK", RAM);
"""

MEMORY_X_D0 = """MEMORY
{
    PSRAM : ORIGIN = 0x50100000, LENGTH = 1M
}
REGION_ALIAS("REGION_TEXT", PSRAM);
REGION_ALIAS("REGION_RODATA", PSRAM);
REGION_ALIAS("REGION_DATA", PSRAM);
REGION_ALIAS("REGION_BSS", PSRAM);
REGION_ALIAS("REGION_HEAP", PSRAM);
REGION_ALIAS("REGION_STACK", PSRAM);
"""

MEMORY_X_LP = """MEMORY
{
    RAM : ORIGIN = 0x22014000, LENGTH = 16K
}
REGION_ALIAS("REGION_TEXT", RAM);
REGION_ALIAS("REGION_RODATA", RAM);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);
REGION_ALIAS("REGION_HEAP", RAM);
REGION_ALIAS("REGION_STACK", RAM);
"""

BUILD_RS = """use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("memory.x"), include_bytes!("memory.x")).unwrap();
    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=build.rs");
}
"""

def make_cargo_toml(name, core_feature):
    return f"""[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
bl808-hal = {{ path = "../../crates/bl808-hal", features = ["{core_feature}", "rt"] }}
riscv = "0.16.1"
riscv-rt = "0.18.0"
panic-halt = "1.0.0"
embedded-hal = "1.0"
"""

M0_EXAMPLES = {
    "m0-hal-gpio": {
        "desc": "GPIO Pin Push-Pull Output & Floating Input Control",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (_clocks, pins) = dp.glb.split();
    let mut led = pins.P8.into_push_pull_output();
    let _ = led.set_low();
    let _ = led.set_high();
    let mut btn = pins.P14.into_floating_input();
    let _ = btn.is_high();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-uart": {
        "desc": "UART0 Serial Driver & Formatted Text Transmission",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use core::fmt::Write;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let tx = pins.P14.into_alternate::<7>();
    let rx = pins.P15.into_alternate::<7>();
    let mut serial = Serial::new(dp.uart0, tx, rx, 2000000, clocks.bclk);
    let _ = write!(serial, "HAL UART M0 initialized @ 2MBaud\\r\\n");
    serial.write_bytes(b"Ready\\r\\n");

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-timer": {
        "desc": "Timer0 Hardware Microsecond & Millisecond Delays",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut timer = Timer::new(dp.timer0);
    let start = timer.get_counter();
    timer.delay_ms(10);
    let elapsed = timer.get_counter().wrapping_sub(start);
    let _ = elapsed;

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-i2c": {
        "desc": "I2C0 Master Bus Initialisation & Device Read",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, _pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let mut i2c = I2c::new(dp.i2c0, &clocks, 100_000);
    let mut buf = [0u8; 1];
    let _ = i2c.read(0x50, &mut buf);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-spi": {
        "desc": "SPI0 Master Full-Duplex Bus Transfer",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, _pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let mut spi = Spi::new(dp.spi0, &clocks, 1_000_000, Polarity::IdleLow, Phase::CaptureOnFirstTransition, BitOrder::MsbFirst);
    let tx_data = [0x9Fu8];
    let mut rx_data = [0u8; 3];
    let _ = spi.transfer(&mut rx_data, &tx_data);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-pwm": {
        "desc": "PWM Channel Grouping, Duty Cycle & Period Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let driver = PwmDriver::new(dp.pwm);
    let (mut grp0, _grp1) = driver.split();
    grp0.set_period(1000);
    grp0.set_duty(0, 500);
    grp0.enable();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-dma": {
        "desc": "DMA0 Memory-to-Memory High-Speed Transfer",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use bl808_hal::dma::{BurstSize, TransferWidth};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let dma = Dma::new(dp.dma0);
    let mut channels = dma.split();
    let mut ch0 = channels.ch0.take().unwrap();

    let src = [0x11223344u32, 0x55667788u32];
    let mut dst = [0u32; 2];
    unsafe {
        let _ = ch0.transfer_mem2mem(&src, &mut dst, TransferWidth::Bits32, BurstSize::Burst1);
    }
    let _ = ch0.wait_transfer();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-ipc": {
        "desc": "Inter-Processor Signalling (M0 to D0/LP)",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut ipc = Ipc::new(dp.ipc);
    ipc.unmask_m0(0x0001);
    ipc.trigger_d0(0x0001);
    let _pending = ipc.pending_m0();
    ipc.clear_m0(0x0001);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-sec": {
        "desc": "Security Engine AES Key Load & SHA-256 Hash Trigger",
        "code": """#![no_std]
#![no_main]

use bl808_hal::sec::Sec;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut sec = Sec::new(dp.sec_eng);
    let key = [0x01020304u32, 0x05060708, 0x090A0B0C, 0x0D0E0F10];
    sec.load_aes_key(&key);
    sec.trigger_sha256();
    let _busy = sec.is_busy();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-hbn": {
        "desc": "Hibernate Domain RTC Control & Root Clock Switching",
        "code": """#![no_std]
#![no_main]

use bl808_hal::hbn::Hbn;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut hbn = Hbn::new(dp.hbn);
    hbn.set_root_clk_sel(1);
    hbn.enable_rtc();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-pds": {
        "desc": "Power-Down Sleep Controller & Wakeup Event Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut pds = Pds::new(dp.pds);
    pds.set_wakeup_sources(WakeupSources::GPIO);
    let _state = pds.state();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-aon": {
        "desc": "Always-On Domain Power Rail & Analog GPADC Config",
        "code": """#![no_std]
#![no_main]

use bl808_hal::aon::Aon;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut aon = Aon::new(dp.aon);
    aon.enable_gpadc_continuous();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-efuse": {
        "desc": "Electronic Fuse Read Trigger & Lock Bit Inspection",
        "code": """#![no_std]
#![no_main]

use bl808_hal::efuse::Efuse;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut efuse = Efuse::new(dp.efuse);
    efuse.trigger_read();
    let _wp = efuse.is_write_protected();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-flash": {
        "desc": "Serial Flash Controller JEDEC ID Readout",
        "code": """#![no_std]
#![no_main]

use bl808_hal::flash::Flash;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut flash = Flash::new(dp.sf_ctrl);
    let _id = flash.read_jedec_id();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-sdh": {
        "desc": "SD Host Controller Interface Initialisation",
        "code": """#![no_std]
#![no_main]

use bl808_hal::sdh::SdhDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let _sdh = SdhDriver::new(dp.sdh);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-emac": {
        "desc": "10/100 Ethernet MAC Descriptor Ring Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::emac::EthernetMac;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut bd_storage = [0u32; 64];
    let _emac = unsafe { EthernetMac::new(dp.emac, bd_storage.as_mut_ptr()) };

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-usb": {
        "desc": "USB 2.0 Device-Mode Controller Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::usb::UsbDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut usb = UsbDriver::new(dp.usb);
    usb.enable_device_mode(true);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-ir": {
        "desc": "Infrared Remote Controller & NEC Protocol Interface",
        "code": """#![no_std]
#![no_main]

use bl808_hal::ir::IrDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let driver = IrDriver::new(dp.ir);
    let (_tx, _rx) = driver.split();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-iso11898": {
        "desc": "CAN Controller Baud Rate Setup & Reset Management",
        "code": """#![no_std]
#![no_main]

use bl808_hal::iso11898::CanDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut can = CanDriver::new(dp.iso11898);
    can.set_reset_mode(true);
    can.set_baud_rate(4, 1, 5, 2, false);
    can.set_reset_mode(false);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-lz4d": {
        "desc": "Hardware LZ4 Decompressor Engine Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::lz4d::Lz4d;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let _lz4d = Lz4d::new(dp.lz4d);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-adc": {
        "desc": "General-Purpose Analog-to-Digital Converter Sampling",
        "code": """#![no_std]
#![no_main]

use bl808_hal::adc::{Adc, AdcChannel};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut adc = Adc::new(dp.aon, dp.gpip);
    let _val = adc.read(AdcChannel::Ch0);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-audio": {
        "desc": "Audio Subsystem Sample-Rate Clock Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::audio::{AudioDriver, ClockConfig};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut audio = AudioDriver::new(dp.audio);
    audio.set_clock(ClockConfig::Rate48k);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-i2s": {
        "desc": "I2S Digital Audio Interface TX/RX Channel Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::i2s::I2sDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let driver = I2sDriver::new(dp.i2s);
    let (mut tx, _rx) = driver.split();
    tx.enable();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-cci": {
        "desc": "Camera Control Interface Serial Clock Configuration",
        "code": """#![no_std]
#![no_main]

use bl808_hal::cci::Cci;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut cci = Cci::new(dp.cci);
    cci.enable();
    cci.set_clock_div(1);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-pta": {
        "desc": "Packet Traffic Arbitration Policy Configuration",
        "code": """#![no_std]
#![no_main]

use bl808_hal::pta::PtaDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut pta = PtaDriver::new(dp.pta);
    let _rev = pta.get_revision();
    pta.set_config_0(0x00000001);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-tsen": {
        "desc": "Internal Temperature Sensor Calibration & Conversion",
        "code": """#![no_std]
#![no_main]

use bl808_hal::tsen::TsenDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let mut tsen = TsenDriver::new();
    tsen.enable();
    tsen.start_conversion();
    let _raw = tsen.read_raw();
    let _temp = tsen.read_decicelsius();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-wifi": {
        "desc": "WiFi Subsystem Coexistence Arbitration Enable",
        "code": """#![no_std]
#![no_main]

use bl808_hal::wifi::WifiDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut wifi = WifiDriver::new(dp.wifi);
    wifi.enable_coex();
    let _state = wifi.get_coex_state();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "m0-hal-plic": {
        "desc": "PLIC Interrupt Controller Context 0 Priority & Enable",
        "code": """#![no_std]
#![no_main]

use bl808_hal::interrupts::{Plic, PLIC_BASE};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let plic = unsafe { Plic::new(PLIC_BASE, 0) };
    unsafe {
        plic.set_priority(1, 1);
        plic.enable(1);
    }

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
}

D0_EXAMPLES = {
    "d0-hal-gpio": {
        "desc": "D0 GLB GPIO Output LED Driving",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (_clk, pins) = dp.glb.split();
    let mut led = pins.P8.into_push_pull_output();
    let _ = led.set_low();
    let _ = led.set_high();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-uart": {
        "desc": "D0 UART3 Serial Console Transmission",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use core::fmt::Write;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let tx = pins.P16.into_alternate::<7>();
    let rx = pins.P17.into_alternate::<7>();
    let mut serial = Serial::new(dp.uart3, tx, rx, 2000000, clocks.bclk);
    let _ = write!(serial, "D0 HAL UART3 initialized\\r\\n");

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-timer": {
        "desc": "D0 Timer1 Hardware Millisecond Delays",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut timer = Timer::new(dp.timer1);
    timer.delay_ms(50);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-i2c": {
        "desc": "D0 Multimedia I2C2 Fast-Mode Master",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, _pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let mut i2c = I2c::new(dp.i2c2, &clocks, 400_000);
    let mut buf = [0u8; 1];
    let _ = i2c.read(0x50, &mut buf);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-spi": {
        "desc": "D0 Multimedia SPI1 Master High-Speed Transfer",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, _pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let mut spi = Spi::new(dp.spi1, &clocks, 2_000_000, Polarity::IdleLow, Phase::CaptureOnFirstTransition, BitOrder::MsbFirst);
    let data = [0xAAu8];
    let mut rx = [0u8; 1];
    let _ = spi.transfer(&mut rx, &data);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-pwm": {
        "desc": "D0 PWM Channel Period & Duty Cycle Control",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let driver = PwmDriver::new(dp.pwm);
    let (mut grp0, _grp1) = driver.split();
    grp0.set_period(2000);
    grp0.set_duty(1, 1000);
    grp0.enable();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-dma": {
        "desc": "D0 Multimedia DMA2 Controller Initialization",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let dma = Dma::new(dp.dma2);
    let _channels = dma.split();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-ipc": {
        "desc": "D0 IPC Communication & Handshake with M0",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut ipc = Ipc::new(dp.ipc);
    ipc.unmask_d0(0x0002);
    ipc.trigger_m0(0x0002);
    let _pending = ipc.pending_d0();
    ipc.clear_d0(0x0002);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-mmglb": {
        "desc": "D0 Multimedia GLB Clock Gating & Reset Control",
        "code": """#![no_std]
#![no_main]

use bl808_hal::mmglb::MmGlb;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut mmglb = MmGlb::new(dp.mm_glb);
    mmglb.enable_codec_clock();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-psram": {
        "desc": "D0 Ultra-High-Speed PSRAM Controller Timing Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::psram::{PsramUhs, UhsMemSize, UhsPageSize, UhsTemp};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut psram = PsramUhs::new(dp.psram_uhs);
    psram.init(2000, UhsMemSize::Mb64, UhsPageSize::Kb2, UhsTemp::Normal);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-codec": {
        "desc": "D0 Codec Subsystem Clock Enable & QoS Control",
        "code": """#![no_std]
#![no_main]

use bl808_hal::codec::{Codec, PCLK_ALL};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut codec = Codec::new(dp.codec);
    codec.enable();
    codec.force_clocks_on(PCLK_ALL);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-dsi": {
        "desc": "D0 Display Serial Interface (MIPI DSI) Transmitter",
        "code": """#![no_std]
#![no_main]

use bl808_hal::dsi::{DsiDriver, DsiLanes};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let mut dsi = DsiDriver::new();
    dsi.set_lane_count(DsiLanes::Two);
    dsi.enable();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-dvp": {
        "desc": "D0 Digital Video Port Camera Frame Capture Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::dvp::{Config, Dvp, Polarity};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut dvp = Dvp::new(dp.dvp0);
    dvp.init(Config {
        vsync_polarity: Polarity::ActiveHigh,
        hsync_polarity: Polarity::ActiveHigh,
        pclk_polarity: Polarity::ActiveHigh,
        width: 640,
        height: 480,
    });

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-h264": {
        "desc": "D0 H.264 Video Encoder Clock & Sequence Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::h264::H264Encoder;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut enc = H264Encoder::new(dp.h264);
    enc.enable_clocks();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-mjpeg": {
        "desc": "D0 Motion JPEG Video Encoder Initialisation",
        "code": """#![no_std]
#![no_main]

use bl808_hal::mjpeg::{Config, MjpegEncoder, YuvFormat};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut mjpeg = MjpegEncoder::new(dp.mjpeg);
    mjpeg.init(Config {
        width: 640,
        height: 480,
        format: YuvFormat::Yuv422,
    });

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-npu": {
        "desc": "D0 Neural Processing Unit Control Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::npu::Npu;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut npu = Npu::new(dp.npu);
    npu.stop();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-mipi": {
        "desc": "D0 MIPI CSI Camera Serial Interface Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::mipi::MipiDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let mut mipi = MipiDriver::new();
    mipi.set_dual_lane(true);
    mipi.enable();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-isp": {
        "desc": "D0 Image Signal Processor Pipeline Configuration",
        "code": """#![no_std]
#![no_main]

use bl808_hal::isp::IspDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let mut isp = IspDriver::new();
    isp.enable_pipeline();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-audio": {
        "desc": "D0 Audio Subsystem Clock Rate Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::audio::{AudioDriver, ClockConfig};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut audio = AudioDriver::new(dp.audio);
    audio.set_clock(ClockConfig::Rate44_1k);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-lz4d": {
        "desc": "D0 Hardware LZ4 Decompressor Engine Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::lz4d::Lz4d;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let _lz4d = Lz4d::new(dp.lz4d);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-pds": {
        "desc": "D0 Power-Down Sleep Wakeup Configuration",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut pds = Pds::new(dp.pds);
    pds.set_wakeup_sources(WakeupSources::GPIO);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-pta": {
        "desc": "D0 Packet Traffic Arbitration Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::pta::PtaDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut pta = PtaDriver::new(dp.pta);
    pta.set_config_0(0x00000001);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "d0-hal-plic": {
        "desc": "D0 PLIC Context 1 Priority & Threshold Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::interrupts::{Plic, PLIC_BASE};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let plic = unsafe { Plic::new(PLIC_BASE, 1) };
    unsafe {
        plic.set_priority(1, 1);
        plic.enable(1);
    }

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
}

LP_EXAMPLES = {
    "lp-hal-gpio": {
        "desc": "LP Core GPIO LED Output Control",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (_clk, pins) = dp.glb.split();
    let mut led = pins.P8.into_push_pull_output();
    let _ = led.set_low();
    let _ = led.set_high();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "lp-hal-uart": {
        "desc": "LP Core UART0 Serial Output",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use core::fmt::Write;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let tx = pins.P14.into_alternate::<7>();
    let rx = pins.P15.into_alternate::<7>();
    let mut serial = Serial::new(dp.uart0, tx, rx, 2000000, clocks.bclk);
    let _ = write!(serial, "LP Core UART0 Ready\\r\\n");

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "lp-hal-timer": {
        "desc": "LP Core Timer0 Delay Generation",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut timer = Timer::new(dp.timer0);
    timer.delay_ms(10);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "lp-hal-i2c": {
        "desc": "LP Core I2C0 Bus Master Read",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, _pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let mut i2c = I2c::new(dp.i2c0, &clocks, 100_000);
    let mut buf = [0u8; 1];
    let _ = i2c.read(0x50, &mut buf);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "lp-hal-spi": {
        "desc": "LP Core SPI0 Master Transfer",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let (clk_ctrl, _pins) = dp.glb.split();
    let clocks = clk_ctrl.get_clocks();
    let mut spi = Spi::new(dp.spi0, &clocks, 1_000_000, Polarity::IdleLow, Phase::CaptureOnFirstTransition, BitOrder::MsbFirst);
    let data = [0x55u8];
    let mut rx = [0u8; 1];
    let _ = spi.transfer(&mut rx, &data);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "lp-hal-pwm": {
        "desc": "LP Core PWM Channel Control",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let driver = PwmDriver::new(dp.pwm);
    let (mut grp0, _grp1) = driver.split();
    grp0.set_period(1000);
    grp0.set_duty(0, 500);
    grp0.enable();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "lp-hal-dma": {
        "desc": "LP Core DMA0 Channel Setup",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let dma = Dma::new(dp.dma0);
    let _channels = dma.split();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "lp-hal-ipc": {
        "desc": "LP Core IPC Mailbox Signalling to M0/D0",
        "code": """#![no_std]
#![no_main]

use bl808_hal::prelude::*;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let dp = bl808_hal::pac::Peripherals::take().unwrap();
    let mut ipc = Ipc::new(dp.ipc);
    ipc.trigger_m0(0x0004);
    ipc.trigger_d0(0x0004);

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "lp-hal-plic": {
        "desc": "LP Core PLIC Context 0 Priority & Enable",
        "code": """#![no_std]
#![no_main]

use bl808_hal::interrupts::{Plic, PLIC_BASE};
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let plic = unsafe { Plic::new(PLIC_BASE, 0) };
    unsafe {
        plic.set_priority(1, 1);
        plic.enable(1);
    }

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
    "lp-hal-tsen": {
        "desc": "LP Core Temperature Sensor Conversion",
        "code": """#![no_std]
#![no_main]

use bl808_hal::tsen::TsenDriver;
use panic_halt as _;
use riscv_rt::entry;

#[entry]
fn main() -> ! {
    let mut tsen = TsenDriver::new();
    tsen.enable();
    tsen.start_conversion();
    let _raw = tsen.read_raw();

    loop {
        core::hint::spin_loop();
    }
}
"""
    },
}

def create_example(name, info, core_feature, memory_x):
    example_dir = os.path.join(EXAMPLES_DIR, name)
    src_dir = os.path.join(example_dir, "src")
    os.makedirs(src_dir, exist_ok=True)

    with open(os.path.join(example_dir, "Cargo.toml"), "w") as f:
        f.write(make_cargo_toml(name, core_feature))

    with open(os.path.join(example_dir, "memory.x"), "w") as f:
        f.write(memory_x)

    with open(os.path.join(example_dir, "build.rs"), "w") as f:
        f.write(BUILD_RS)

    with open(os.path.join(src_dir, "main.rs"), "w") as f:
        f.write(info["code"])

    print(f"Created {name}: {info['desc']}")

def main():
    os.makedirs(EXAMPLES_DIR, exist_ok=True)

    print("Generating M0 HAL examples...")
    for name, info in M0_EXAMPLES.items():
        create_example(name, info, "m0", MEMORY_X_M0)

    print("\\nGenerating D0 HAL examples...")
    for name, info in D0_EXAMPLES.items():
        create_example(name, info, "d0", MEMORY_X_D0)

    print("\\nGenerating LP HAL examples...")
    for name, info in LP_EXAMPLES.items():
        create_example(name, info, "lp", MEMORY_X_LP)

    total = len(M0_EXAMPLES) + len(D0_EXAMPLES) + len(LP_EXAMPLES)
    print(f"\\nGenerated {total} HAL examples successfully.")

if __name__ == "__main__":
    main()
