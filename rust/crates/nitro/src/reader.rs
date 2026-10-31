//! The big-endian `BinaryReader` the Nitro code used for .nitro buffers.

use anyhow::{bail, Result};

pub struct BinaryReader<'a> {
    position: usize,
    buffer: &'a [u8],
}

impl<'a> BinaryReader<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self { position: 0, buffer }
    }

    pub fn read_byte(&mut self) -> Result<u8> {
        Ok(self.read_bytes(1)?[0])
    }

    pub fn read_bytes(&mut self, length: usize) -> Result<&'a [u8]> {
        if length > self.remaining() {
            bail!("Unexpected end of buffer: wanted {length} bytes, {} left.", self.remaining());
        }
        let slice = &self.buffer[self.position..self.position + length];
        self.position += length;
        Ok(slice)
    }

    pub fn read_short(&mut self) -> Result<i16> {
        let bytes = self.read_bytes(2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub fn read_int(&mut self) -> Result<i32> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn remaining(&self) -> usize {
        self.buffer.len() - self.position
    }

    pub fn position(&self) -> usize {
        self.position
    }

    /// The rest of the buffer as UTF-8 (lossy).
    pub fn rest_to_string(&self) -> String {
        String::from_utf8_lossy(&self.buffer[self.position..]).into_owned()
    }

    pub fn buffer(&self) -> &'a [u8] {
        self.buffer
    }
}
