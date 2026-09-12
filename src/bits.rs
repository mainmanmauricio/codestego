//! Bit-level reader and writer (MSB-first within each byte).

#[derive(Debug, Clone, Default)]
pub struct BitWriter {
    buf: Vec<u8>,
    bit_pos: u8, // 0..7, next bit index within current incomplete byte (MSB=0)
}

impl BitWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_bit(&mut self, bit: bool) {
        if self.bit_pos == 0 {
            self.buf.push(0);
        }
        let last = self.buf.len() - 1;
        if bit {
            self.buf[last] |= 1 << (7 - self.bit_pos);
        }
        self.bit_pos = (self.bit_pos + 1) % 8;
    }

    pub fn write_bits(&mut self, value: u64, nbits: usize) {
        for i in (0..nbits).rev() {
            self.write_bit(((value >> i) & 1) == 1);
        }
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_bits(b as u64, 8);
        }
    }

    pub fn len_bits(&self) -> usize {
        if self.buf.is_empty() {
            0
        } else if self.bit_pos == 0 {
            self.buf.len() * 8
        } else {
            (self.buf.len() - 1) * 8 + self.bit_pos as usize
        }
    }

    /// Finish and return bytes (zero-padded in the final partial byte).
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }
}

#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Absolute bit offset from start of data.
    pos: usize,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn from_offset(data: &'a [u8], bit_offset: usize) -> Self {
        Self {
            data,
            pos: bit_offset,
        }
    }

    pub fn remaining_bits(&self) -> usize {
        (self.data.len() * 8).saturating_sub(self.pos)
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn read_bit(&mut self) -> Option<bool> {
        if self.pos >= self.data.len() * 8 {
            return None;
        }
        let byte = self.data[self.pos / 8];
        let bit = (byte >> (7 - (self.pos % 8))) & 1;
        self.pos += 1;
        Some(bit == 1)
    }

    pub fn read_bits(&mut self, nbits: usize) -> Option<u64> {
        if nbits > 64 {
            return None;
        }
        let mut v = 0u64;
        for _ in 0..nbits {
            let b = self.read_bit()?;
            v = (v << 1) | (b as u64);
        }
        Some(v)
    }

    pub fn read_bytes(&mut self, nbytes: usize) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(nbytes);
        for _ in 0..nbytes {
            out.push(self.read_bits(8)? as u8);
        }
        Some(out)
    }

    pub fn skip_bits(&mut self, n: usize) -> bool {
        if self.pos + n > self.data.len() * 8 {
            return false;
        }
        self.pos += n;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_roundtrip() {
        let mut w = BitWriter::new();
        w.write_bits(0b1011_0100, 8);
        w.write_bit(true);
        w.write_bit(false);
        w.write_bits(0b111, 3);
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read_bits(8).unwrap(), 0b1011_0100);
        assert!(r.read_bit().unwrap());
        assert!(!r.read_bit().unwrap());
        assert_eq!(r.read_bits(3).unwrap(), 0b111);
    }

    #[test]
    fn byte_aligned() {
        let data = vec![0xAB, 0xCD];
        let mut w = BitWriter::new();
        w.write_bytes(&data);
        assert_eq!(w.finish(), data);
    }
}
