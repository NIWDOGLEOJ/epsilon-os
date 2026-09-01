//! 16550 UART Serial Driver for COM1 (0x3F8)
//!
//! Provides thread-safe serial console logging and formatted output macros.

use core::fmt::{self, Write};
use spin::Mutex;

/// Standard COM1 Base I/O Port
pub const COM1_BASE: u16 = 0x3F8;

// Register Offsets from Base
const DATA_PORT: u16 = 0;     // Transmit/Receive Buffer (or DLL when DLAB=1)
const INT_ENABLE: u16 = 1;    // Interrupt Enable Register (or DLM when DLAB=1)
const FIFO_CTRL: u16 = 2;     // FIFO Control Register (Write) / IIR (Read)
const LINE_CTRL: u16 = 3;     // Line Control Register (DLAB bit 7)
const MODEM_CTRL: u16 = 4;    // Modem Control Register
const LINE_STATUS: u16 = 5;   // Line Status Register

// Line Status Register Bits
const LSR_DATA_READY: u8 = 0x01;
const LSR_TRANSMITTER_EMPTY: u8 = 0x20;

/// Low-level x86 Port I/O: Read Byte
#[inline(always)]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

/// Low-level x86 Port I/O: Write Byte
#[inline(always)]
pub unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}

/// Short I/O delay to allow legacy bus settling
#[inline(always)]
pub unsafe fn io_wait() {
    outb(0x80, 0);
}

/// 16550 UART Controller Interface
pub struct SerialPort {
    base: u16,
    initialized: bool,
}

impl SerialPort {
    /// Create a new SerialPort bound to the given base port address
    pub const fn new(base: u16) -> Self {
        Self {
            base,
            initialized: false,
        }
    }

    /// Initialize the 16550 UART chip
    pub fn init(&mut self) {
        unsafe {
            // 1. Disable all serial interrupts during configuration
            outb(self.base + INT_ENABLE, 0x00);
            io_wait();

            // 2. Enable DLAB (Divisor Latch Access Bit) in Line Control Register
            outb(self.base + LINE_CTRL, 0x80);
            io_wait();

            // 3. Set baud rate divisor to 1 (115,200 Baud)
            // Divisor = 115200 / 115200 = 1 (LSB = 0x01, MSB = 0x00)
            outb(self.base + DATA_PORT, 0x01); // Divisor LSB
            io_wait();
            outb(self.base + INT_ENABLE, 0x00); // Divisor MSB
            io_wait();

            // 4. Configure Line Control: 8 data bits, 1 stop bit, no parity (8N1), clear DLAB
            outb(self.base + LINE_CTRL, 0x03);
            io_wait();

            // 5. Enable FIFO, clear TX/RX queues, set 14-byte interrupt threshold
            outb(self.base + FIFO_CTRL, 0xC7);
            io_wait();

            // 6. Configure Modem Control: Set RTS/DSR, enable Auxiliary Output 2 (OUT2)
            outb(self.base + MODEM_CTRL, 0x0B);
            io_wait();

            // 7. Loopback test to verify hardware transceiver
            outb(self.base + MODEM_CTRL, 0x1E); // Enable loopback mode
            io_wait();
            outb(self.base + DATA_PORT, 0xAE); // Send test byte
            io_wait();
            let _ = inb(self.base + DATA_PORT); // Read back byte

            // 8. Restore normal operation mode (disable loopback, IRQs active, OUT1/OUT2 set)
            outb(self.base + MODEM_CTRL, 0x0F);
            io_wait();
        }
        self.initialized = true;
    }

    /// Check if the transmit FIFO buffer is empty and ready for new data
    #[inline]
    pub fn is_transmit_empty(&self) -> bool {
        unsafe { (inb(self.base + LINE_STATUS) & LSR_TRANSMITTER_EMPTY) != 0 }
    }

    /// Write a single raw byte to the serial port
    pub fn write_byte(&mut self, byte: u8) {
        while !self.is_transmit_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(self.base + DATA_PORT, byte);
        }
    }

    /// Write a string slice to serial with automatic \n -> \r\n line ending conversion
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
    }

    /// Check if data is available to read from the serial port
    #[inline]
    pub fn is_data_ready(&self) -> bool {
        unsafe { (inb(self.base + LINE_STATUS) & LSR_DATA_READY) != 0 }
    }

    /// Read a single byte from the serial port if available
    pub fn read_byte(&mut self) -> Option<u8> {
        if self.is_data_ready() {
            Some(unsafe { inb(self.base + DATA_PORT) })
        } else {
            None
        }
    }
}

impl Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

/// Global synchronized COM1 serial port singleton
pub static SERIAL1: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM1_BASE));

/// Initialize the global COM1 serial console
pub fn init_serial() {
    SERIAL1.lock().init();
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    // Held across the lock: an ISR that logs would otherwise deadlock on SERIAL1.
    let _guard = crate::arch::InterruptGuard::acquire();
    let mut serial = SERIAL1.lock();
    let _ = serial.write_fmt(args);
}

/// Print formatted text to the COM1 serial console
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::arch::serial::_print(format_args!($($arg)*))
    };
}

/// Print formatted text with trailing newline to the COM1 serial console
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::print!("{}\n", format_args!($($arg)*))
    };
}

/// Explicit alias for serial printing
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::arch::serial::_print(format_args!($($arg)*))
    };
}

/// Explicit alias for serial printing with trailing newline
#[macro_export]
macro_rules! serial_println {
    () => {
        $crate::serial_print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::serial_print!("{}\n", format_args!($($arg)*))
    };
}
