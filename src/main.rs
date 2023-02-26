#![no_std]
#![no_main]

#![feature(panic_info_message)]

mod sync;
mod uart;

use core::arch::{asm, global_asm};
use core::fmt::Write;
use core::mem::MaybeUninit;
use core::ops::Range;
use core::panic::PanicInfo;
use core::write;

use self::uart::UART;

/// Peripherals range.
const PERRY_RANGE: Range<usize> = 0x80000000 .. 0x84000000;
/// Logical CPU count.
const CPU_COUNT: usize = 4;

global_asm!(include_str!("boot.s"));

/// Entry point.
#[no_mangle]
pub extern "C" fn start() -> !
{
    let cpu = cpu_id();
    debug!("Booted core #{cpu}");
    bench();
    halt()
}

/// Benchmarks.
fn bench()
{
    #[repr(align(64), C)]
    struct Buffer([u8; 0x1000]);
    let mut buf = MaybeUninit::<Buffer>::uninit();
    unsafe {
        asm!(
            "add {eaddr}, {addr}, #0x1000",
            "0:",
            "cmp {addr}, {eaddr}",
            "beq 0f",
            "prfm pstl1keep, [{addr}]",
            "add {addr}, {addr}, #64",
            "b 0b",
            "0:",
            addr = inout (reg) buf.as_mut_ptr() => _,
            eaddr = out (reg) _,
        );
    }
    let start: usize;
    unsafe {
        asm!(
            "mrs {now}, cntpct_el0",
            now = out (reg) start,
            options (nomem, nostack, preserves_flags)
        );
    }
    for _ in 0 .. 2 << 20 {
        unsafe {
            asm!(
                "add {eaddr}, {addr}, #0x1000",
                "ins {data}.d[0], xzr",
                "ins {data}.d[1], xzr",
                "0:",
                "cmp {addr}, {eaddr}",
                "beq 0f",
                "stp {data:q}, {data:q}, [{addr}], #32",
                "b 0b",
                "0:",
                addr = inout (reg) buf.as_mut_ptr() => _,
                eaddr = out (reg) _,
                data = out (vreg) _
            );
        }
    }
    let end: usize;
    let freq: usize;
    unsafe {
        asm!(
            "mrs {now}, cntpct_el0",
            "mrs {freq}, cntfrq_el0",
            now = out (reg) end,
            freq = out (reg) freq,
            options (nomem, nostack, preserves_flags)
        );
    }
    let diff = end - start;
    let secs = diff / freq;
    let msecs = diff / (freq / 1000) % 1000;
    let core = cpu_id();
    debug!("Core #{core} wrote 8GB in {secs}.{msecs:03} secs");
}

/// Panics with diagnostic information about a fault.
#[no_mangle]
pub extern "C" fn fault(kind: usize) -> !
{
    let core = cpu_id();
    let level: usize;
    let syndrome: usize;
    let addr: usize;
    let ret: usize;
    let state: usize;
    unsafe {
        asm!(
            "mrs {el}, currentel",
            "lsr {el}, {el}, #2",
            el = out (reg) level,
            options (nomem, nostack, preserves_flags));
        match level {
            2 => asm!(
                    "mrs {synd}, esr_el2",
                    "mrs {addr}, far_el2",
                    "mrs {ret}, elr_el2",
                    "mrs {state}, spsr_el2",
                    synd = out (reg) syndrome,
                    addr = out (reg) addr,
                    ret = out (reg) ret,
                    state = out (reg) state,
                    options (nomem, nostack, preserves_flags)),
            1 => asm!(
                    "mrs {synd}, esr_el1",
                    "mrs {addr}, far_el1",
                    "mrs {ret}, elr_el1",
                    "mrs {state}, spsr_el1",
                    synd = out (reg) syndrome,
                    addr = out (reg) addr,
                    ret = out (reg) ret,
                    state = out (reg) state,
                    options (nomem, nostack, preserves_flags)),
            _ => panic!("Exception caught at unsupported level {level}"),
        }
    };
    panic!("Core #{core} triggered an exception at level {level}: Kind: 0x{kind:x}, Syndrome: 0x{syndrome:x}, Address: 0x{addr:x}, Location: 0x{ret:x}, State: 0x{state:x}");
}

/// Halts the calling core.
#[no_mangle]
pub extern "C" fn halt() -> !
{
    let core = cpu_id();
    debug!("Halted core #{core}");
    unsafe {
        asm!("msr daifset, #0x3",
             "0:",
             "wfi",
             "b 0b",
             options(nomem, nostack, preserves_flags, noreturn))
    }
}

/// Halts the system with a diagnostic error message.
#[panic_handler]
fn panic(info: &PanicInfo) -> !
{
    let mut uart = UART.lock();
    let affinity = cpu_id();
    if let Some(location) = info.location() {
        write!(uart,
               "Core #{affinity} panicked at {}:{}: ",
               location.file(),
               location.line()).unwrap()
    } else {
        write!(uart, "Core #{affinity} panic: ").unwrap()
    }
    if let Some(args) = info.message() {
        uart.write_fmt(*args).unwrap()
    } else {
        uart.write_str("Unknown reason").unwrap()
    }
    uart.write_char('\n').unwrap();
    drop(uart);
    backtrace();
    halt();
}

/// Returns the ID of the current CPU core.
fn cpu_id() -> usize
{
    let id: usize;
    unsafe {
        asm!(
            "mrs {id}, mpidr_el1",
            "and {id}, {id}, #0xff",
            id = out (reg) id,
            options (nomem, nostack, preserves_flags));
    }
    id
}

/// Sends the return addresses of all the function calls from this function all
/// the way back to the boot code through the UART.
fn backtrace()
{
    let mut uart = UART.lock();
    let mut fp: usize;
    let mut lr: usize;
    unsafe {
        asm!("mov {fp}, fp", "mov {lr}, lr", fp = out (reg) fp, lr = out (reg) lr, options (nomem, nostack, preserves_flags))
    };
    let mut frame = 0usize;
    writeln!(uart, "Backtrace:").unwrap();
    while fp != 0x0 {
        writeln!(uart, "#{frame}: 0x{lr:X}").unwrap();
        unsafe { asm!("ldp {fp}, {lr}, [{fp}]", fp = inout (reg) fp, lr = out (reg) lr, options (preserves_flags)) };
        frame += 1;
    }
}
