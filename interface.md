./nandpromax <sub> <operation> <i/o> [options]


<sub>:

- nand
- emmc
- xsvf


<operation>:

- detect (xsvf only)
- read (nand/emmc)
- write (nand/emmc/xsvf)

[options]:

NandProMAx

-d, --device <DEVICE>             Hardware device (Priority: picoflasher -> ftdi -> lpc -> demon) [possible values: picoflasher, ftdi, lpc, demon]

--timeout-ms <TIMEOUT_MS>    Operation timeout in milliseconds [default: 3000]

--serial <SERIAL>            USB Serial port (e.g. /dev/ttyACM0)


Read/Write

--start <START>              Start block / LBA offset [default: 0]

--count <COUNT>              Number of blocks / LBAs to read/write


Write

--erase <ERASE>              Erase block before writing [default: true] [possible values: true, false]
--verify                     Verify block after writing


FTDI

--ftdi-desc <FTDI_DESC>      FTDI device description filter [default: auto]

--ftdi-index <FTDI_INDEX>    FTDI device index

--freq-hz <FREQ_HZ>          SPI frequency in Hz [default: 6000000]

--page-format <PAGE_FORMAT>  FTDI page format [default: auto] [possible values: auto, small, big]



<DEVICE>:

- PICO - PicoFlasher v4+ / DirtyPico
- FTDI - xFlasher / Squirt
- LPC - NANDX / MTX
- JRP - JR-Programmer v1/v2
- DEMON - TX Demon
- ESP - ESPFlasher (PicoFlasher TCP)


<ADAPTER>:

