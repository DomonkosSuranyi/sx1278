
mod register;
mod mode;
mod radio_settings;

pub use self::mode::Mode;
pub use self::radio_settings::{RadioSettings, SpreadingFactor, Bandwidth, ErrorCoding};


use core::marker::PhantomData;

use hal::blocking::spi;
use hal::digital::OutputPin;

use self::register::Register;
use ::SX1278;
use ::LoRa;


#[derive(Debug)]
pub enum Error<E> {
    /// Late collision
    TooLargeSymbolTimeout,
    /// SPI error
    Spi(E),
}


impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::Spi(e)
    }
}


impl<E, SPI, NSS> SX1278<SPI, NSS, LoRa>
where
    SPI: spi::Transfer<u8, Error = E>,
    NSS: OutputPin,
{
    pub fn new(spi: SPI, nss: NSS) -> Result<Self, E> {
        let mut sx = SX1278 { spi, nss, _reg_mode: PhantomData };
        sx.initialize()?;
        Ok(sx)
    }

    fn initialize(&mut self) -> Result<(), E> {
        const LONG_RANGE_MODE_MASK: u8 = 0b1000_0000;
        let mut opmode = self.read(Register::OpMode)?;
        if opmode & MODE_MASK != 0b000 {
            // need to switch to sleep mode first
            // as RFMODE can only switched in SLEEP mode (see 6.4)
            opmode = opmode & !MODE_MASK;
            self.write(Register::OpMode, opmode)?;
        }
        opmode = (opmode & !LONG_RANGE_MODE_MASK) | LONG_RANGE_MODE_MASK;
        self.write(Register::OpMode, opmode)
    }

    /// Returns the HW version of the chip (Reg 0x42).
    ///
    /// - MSB nibble: full revision number
    /// - LSB nibble: metal mask revision number
    pub fn version(&mut self) -> Result<u8, E> {
        self.read(Register::Version)
    }

    /// Returns the current Mode
    ///
    /// See [`Mode`].
    pub fn mode(&mut self) -> Result<Mode, E> {
        self.read(Register::OpMode).map(|opmode| Mode::from_opmode(opmode))
    }

    pub fn set_mode(&mut self, new_mode: Mode) -> Result<(), E> {
        let mut opmode = self.read(Register::OpMode)?;
        opmode = (opmode & !MODE_MASK) | (new_mode as u8);
        self.write(Register::OpMode, opmode)
    }

    /// Sets the F_rf value
    ///
    // TODO: format this formula correctly by Pulldown renderer
    /// F_rf = (2^19) * freq / F(XOSC)
    ///
    /// where:
    ///
    ///    - freq: the desired frequency in Hz
    ///    - F(XOSC): the frequency of the crystal oscillator in Hz (default: 32MHz)
    pub fn set_f_rf(&mut self, f_rf: u32) -> Result<(), E> {
        let msb = (f_rf >> 16) & 0xff;
        let mid = (f_rf >> 8) & 0xff;
        let lsb = f_rf & 0xff;
        self.write(Register::FreqMsb, msb as u8)?; // TODO write burst
        self.write(Register::FreqMid, mid as u8)?;
        self.write(Register::FreqLsb, lsb as u8)
    }

    pub fn set_payload_length(&mut self, length: u8) -> Result<(), E> {
        self.write(Register::PayloadLength, length)
    }

    /// Sets [`RadioSettings`]
    ///
    /// Note: it also clears the TXCONTINUOUS flag (it is used only for spectral testing).
    /// See [`RadioSettings`]
    pub fn set_radio_settings(&mut self, settings: &RadioSettings) -> Result<(), Error<E>> {
        if settings.symbol_timeout >= 1024 {
            return Err(Error::TooLargeSymbolTimeout);
        }

        let config1 = (settings.bandwidth as u8) << 4 |
            (settings.error_coding as u8) << 1 |
            (settings.implicit_header as u8);

        self.write(Register::ModemConfig1, config1)?;

        let config2 = (settings.spreading_factor as u8) << 4 |
            (settings.crc as u8) << 2 |
            (settings.symbol_timeout >> 8) as u8;
        self.write(Register::ModemConfig2, config2)?;
        self.write(Register::SymbTimeoutLsb, (settings.symbol_timeout & 0xff) as u8)?;

        self.set_payload_length(settings.payload_length)?;
        self.set_f_rf(settings.f_rf)?;

        let mut config3 = self.read(Register::ModemConfig3)?;
        config3 = config3 & !0b0000_1000 | (settings.low_datarate_optimize as u8) << 3;
        self.write(Register::ModemConfig3, config3)?;
        Ok(())
    }

    // bus
    fn read(&mut self, reg: Register) -> Result<u8, E> {
        let mut buffer = [0x00 | reg.addr(), 0];
        self.nss.set_low();
        let buffer = self.spi.transfer(&mut buffer)?;
        self.nss.set_high();
        Ok(buffer[1])
    }

    fn write(&mut self, reg: Register, value: u8) -> Result<(), E> {
        let mut buffer = [0x80 | reg.addr(), value];
        self.nss.set_low();
        self.spi.transfer(&mut buffer)?;
        self.nss.set_high();
        Ok(())
    }
}

const MODE_MASK: u8 = 0b0000_0111;