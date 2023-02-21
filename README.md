I'm working on a [bare metal project](https://github.com/Crifrald/nether) for the Raspberry Pi 4 that is not exactly an operating system but has a lot in common with a traditional kernel.  At the moment this project is nothing more than a graphics library stub and is already pretty slow, so following advice that I got on the Raspberry Pi forums I benchmarked it and was negatively impressed by the numbers, especially the fill rate numbers, where I didn't expect any problems since that's just writing to a small tile in memory and at least in theory I should be hitting the L1 cache all the time.

While there's definitely some low hanging fruit to collect such as the fact that the pi boots under-clocked, that doesn't explain everything,  One of the cache-related issues that I experience is that running the benchmark on all 4 cores at the same time produces results that are way worse than when I only run it on a single core.  Not only that but decreasing the size of the buffer and increasing the number of iterations in the same proportion so that the amount of data written remains the same actually improves performance.  And as if this wasn't weird enough, on Linux I get over twice the performance that I'm getting on bare metal running the same code.

The following snippet of Rust code is what I'm using to test the fill rate on both the bare metal and Linux targets, it has some inline assembly because I couldn't get Rust to produce similar assembly output for both targets even though I'm compiling both with `-C opt-level=3`:

        #[repr(align(64), C)]
        struct Buffer([u8; 0x1000]);
        let mut buf = MaybeUninit::<Buffer>::uninit();
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

These are the numbers that I get when I run this code on a single thread on Linux on a Raspberry Pi 4:

    real	0m0.679s
    user	0m0.671s
    sys	0m0.008s

The following is what I get when I run the benchmark on a single core on another Raspberry Pi 4 without an operating system:

    Core #0 wrote 8GB of data in 1.733 seconds

The following is what I get when I run the code on all 4 cores of the same Raspberry Pi 4 as above:

    Core #0 wrote 8GB of data in 12.746 seconds
    Core #2 wrote 8GB of data in 12.751 seconds
    Core #1 wrote 8GB of data in 12.751 seconds
    Core #3 wrote 8GB of data in 12.750 seconds

And the following is what happens when I half the size of the buffer and double the number of iterations:

    Core #0 wrote 8GB of data in 6.125 seconds
    Core #2 wrote 8GB of data in 6.129 seconds
    Core #1 wrote 8GB of data in 6.129 seconds
    Core #3 wrote 8GB of data in 6.128 seconds

I would expect the first two bare metal results if the cache was disabled, because then I'd be hitting the memory bus very hard, but this doesn't explain why halving the size of the buffer and increasing the number of iterations produces better results in the third bare metal benchmark.  In any case I actually went ahead and disabled the cache of the stack areas (where the buffers are located) with the buffer size and number of iterations restored which had the following results:

    Core #0 wrote 8GB of data in 20.483 seconds
    Core #1 wrote 8GB of data in 20.480 seconds
    Core #2 wrote 8GB of data in 20.482 seconds
    Core #3 wrote 8GB of data in 20.485 seconds

This is how I'm setting up the MMU-related registers:

        adrp x0, root_tt
        msr ttbr0_el1, x0
        mov x0, #0x7d01 << 32
        movk x0, #0x809d, lsl #16
        movk x0, #0x3520
        msr tcr_el1, x0
        mov x0, #0x44ff
        msr mair_el1, x0
        mov x0, #0x30d0 << 16
        movk x0, #0x1b9f
        msr sctlr_el1, x0
        isb

All the memory except hardware memory is being mapped as inner and outer write-back non-transient read-allocating write-allocating cacheable (MAIR index 0) with the read-only sections being marked as not shared and the read-write sections being marked as full system shared, except for the stacks which are marked as not shared because Rust guarantees that safe code cannot access code from other threads unless that data is static.

To make sure that Linux isn't doing anything unusual, I wrote a kernel module to report the values in some registers as well as in the first user space page that it could find, and the values are as follows:

    MAIR: 0x000000040044ffff
    TCR: 0x00000074b5593519
    SCTLR: 0x0000000034d4d83d
    Record: 0x00e80000471d0f43

While these numbers vary a lot from my own configuration, which is natural because the projects have different needs, the only thing that I do not understand is the reason for bit 51 to be set on the page record.

Can anyone help me find the reason for this low cache performance?

In case someone is interested in taking a look, I've made the [benchmark code](https://github.com/Crifrald/rpi-bmark) available.