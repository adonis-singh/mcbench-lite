//! A compressed answer decoded as Node 24's fetch (undici 7.18.2 with zlib)
//! decodes it, which is not as strict as `reqwest`'s decoder.
//!
//! undici reads `content-encoding` as a comma list, lower case, and builds
//! one decoder per coding from the last to the first: `gzip` and `x-gzip`
//! a gunzip, `deflate` an inflate (zlib-wrapped when the first byte's low
//! nibble is 8, raw otherwise), and any coding it does not know (identity
//! included) means no decoding at all. Each zlib stream runs with
//! `Z_SYNC_FLUSH` for every write and for the end, so a body that ends
//! early is no error: what was decoded is the body, a gzip member without
//! its trailer, a deflate stream cut inside a block, a bare header. What is
//! an error (zlib's `Z_DATA_ERROR`, which fetch reports as `terminated`): a
//! header that is not gzip or zlib, data that is not deflate, a trailer
//! whose CRC-32 or length (gzip) or Adler-32 (zlib) is wrong, and bytes
//! after a gzip member that are not another member (Node's gunzip starts a
//! new member at any byte but zero; a zero ends the write's input). The
//! output of the write that failed is not delivered, as Node's zlib emits
//! nothing from a write that errs.
//!
//! Measured with Node 24.13.1 over chunked, `content-length` and
//! close-delimited bodies (`tests/node/cases.js`, the `gzip-*` and
//! `deflate-*` streams, run through both sides by the differential test).
//! Brotli and zstd are not asked for (`accept-encoding: gzip, deflate`) and
//! are not decoded here; a provider that sent one unasked would be read
//! raw.

use flate2::{Crc, Decompress, FlushDecompress, Status};

/// The body's decoders, applied in order (the last coding first).
pub struct Decoders {
    chain: Vec<Coding>,
}

/// A body that is not what its encoding says (`Z_DATA_ERROR`).
#[derive(Debug, PartialEq, Eq)]
pub struct DataError;

impl Decoders {
    /// The decoders of an answer with `status` and `content-encoding`.
    pub fn new(content_encoding: Option<&str>, status: u16) -> Decoders {
        let mut chain = Vec::new();
        // A null-body status has nothing to decode.
        if let Some(header) = content_encoding.filter(|_| ![101, 204, 205, 304].contains(&status)) {
            let codings: Vec<String> = header
                .to_ascii_lowercase()
                .split(',')
                .map(|c| c.trim().to_owned())
                .collect();
            for coding in codings.iter().rev() {
                match coding.as_str() {
                    "gzip" | "x-gzip" => chain.push(Coding::Gunzip(Gunzip::default())),
                    "deflate" => chain.push(Coding::Inflate(Inflate::default())),
                    _ => {
                        chain.clear();
                        break;
                    }
                }
            }
        }
        Decoders { chain }
    }

    /// Decodes the next piece of the body.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<u8>, DataError> {
        let mut data = bytes.to_vec();
        for coding in &mut self.chain {
            data = coding.push(&data)?;
        }
        Ok(data)
    }
}

enum Coding {
    Gunzip(Gunzip),
    Inflate(Inflate),
}

impl Coding {
    fn push(&mut self, input: &[u8]) -> Result<Vec<u8>, DataError> {
        match self {
            Coding::Gunzip(g) => g.push(input),
            Coding::Inflate(i) => i.push(input),
        }
    }
}

/// Runs `inflater` over `input` with a sync flush: the bytes it took, the
/// output, and whether the deflate stream ended.
fn inflate(inflater: &mut Decompress, input: &[u8]) -> Result<(usize, Vec<u8>, bool), DataError> {
    let mut out = Vec::with_capacity(input.len() * 4 + 64);
    let start_in = inflater.total_in();
    loop {
        let taken = (inflater.total_in() - start_in) as usize;
        if out.len() == out.capacity() {
            out.reserve(out.capacity().max(4096));
        }
        let before_out = inflater.total_out();
        let status = inflater
            .decompress_vec(&input[taken..], &mut out, FlushDecompress::Sync)
            .map_err(|_| DataError)?;
        let taken = (inflater.total_in() - start_in) as usize;
        match status {
            Status::StreamEnd => return Ok((taken, out, true)),
            Status::Ok | Status::BufError => {
                let progressed = inflater.total_out() > before_out;
                if taken == input.len() && out.len() < out.capacity() {
                    return Ok((taken, out, false));
                }
                if !progressed && out.len() < out.capacity() {
                    return Ok((taken, out, false));
                }
            }
        }
    }
}

#[derive(Default)]
enum GzState {
    #[default]
    Header,
    Body,
    Trailer,
    After,
}

/// `zlib.createGunzip({flush, finishFlush: Z_SYNC_FLUSH})`.
struct Gunzip {
    state: GzState,
    buffer: Vec<u8>,
    inflater: Decompress,
    crc: Crc,
}

impl Default for Gunzip {
    fn default() -> Gunzip {
        Gunzip {
            state: GzState::Header,
            buffer: Vec::new(),
            inflater: Decompress::new(false),
            crc: Crc::new(),
        }
    }
}

/// The length of a whole gzip header at the start of `b`, `None` while it is
/// not all there, or an error for one zlib refuses.
fn gzip_header(b: &[u8]) -> Result<Option<usize>, DataError> {
    let need = |n: usize| b.len() >= n;
    if need(1) && b[0] != 0x1f || need(2) && b[1] != 0x8b {
        return Err(DataError); // incorrect header check
    }
    if need(3) && b[2] != 8 {
        return Err(DataError); // unknown compression method
    }
    if !need(4) {
        return Ok(None);
    }
    let flags = b[3];
    if flags & 0xe0 != 0 {
        return Err(DataError); // unknown header flags set
    }
    let mut at = 10;
    if !need(at) {
        return Ok(None);
    }
    if flags & 0x04 != 0 {
        if !need(at + 2) {
            return Ok(None);
        }
        at += 2 + usize::from(u16::from_le_bytes([b[at], b[at + 1]]));
        if !need(at) {
            return Ok(None);
        }
    }
    for flag in [0x08, 0x10] {
        if flags & flag != 0 {
            match b[at..].iter().position(|&c| c == 0) {
                Some(zero) => at += zero + 1,
                None => return Ok(None),
            }
        }
    }
    if flags & 0x02 != 0 {
        if !need(at + 2) {
            return Ok(None);
        }
        let mut crc = Crc::new();
        crc.update(&b[..at]);
        if u16::from_le_bytes([b[at], b[at + 1]]) != (crc.sum() & 0xffff) as u16 {
            return Err(DataError); // header crc mismatch
        }
        at += 2;
    }
    Ok(Some(at))
}

impl Gunzip {
    fn push(&mut self, input: &[u8]) -> Result<Vec<u8>, DataError> {
        self.buffer.extend_from_slice(input);
        let mut out = Vec::new();
        loop {
            match self.state {
                GzState::Header => match gzip_header(&self.buffer)? {
                    None => break,
                    Some(length) => {
                        self.buffer.drain(..length);
                        self.inflater.reset(false);
                        self.crc.reset();
                        self.state = GzState::Body;
                    }
                },
                GzState::Body => {
                    let (taken, decoded, ended) = inflate(&mut self.inflater, &self.buffer)?;
                    self.buffer.drain(..taken);
                    self.crc.update(&decoded);
                    out.extend_from_slice(&decoded);
                    if !ended {
                        break;
                    }
                    self.state = GzState::Trailer;
                }
                GzState::Trailer => {
                    if self.buffer.len() < 8 {
                        break;
                    }
                    let crc = u32::from_le_bytes(self.buffer[..4].try_into().expect("4 bytes"));
                    let size = u32::from_le_bytes(self.buffer[4..8].try_into().expect("4 bytes"));
                    if crc != self.crc.sum() || size != self.crc.amount() {
                        return Err(DataError); // incorrect data check, incorrect length check
                    }
                    self.buffer.drain(..8);
                    self.state = GzState::After;
                }
                GzState::After => {
                    match self.buffer.first() {
                        None => break,
                        // Trailing zeros (padding) end this write's input.
                        Some(0) => {
                            self.buffer.clear();
                            break;
                        }
                        // Another member, or garbage that fails as one.
                        Some(_) => self.state = GzState::Header,
                    }
                }
            }
        }
        Ok(out)
    }
}

#[derive(Default)]
enum ZState {
    #[default]
    First,
    Header,
    Body,
    Trailer,
    Done,
}

/// undici's `createInflate`: zlib's inflate for a zlib-wrapped stream, its
/// raw inflate otherwise, chosen by the first byte.
struct Inflate {
    state: ZState,
    raw: bool,
    buffer: Vec<u8>,
    inflater: Decompress,
    adler: (u32, u32),
}

impl Default for Inflate {
    fn default() -> Inflate {
        Inflate {
            state: ZState::First,
            raw: false,
            buffer: Vec::new(),
            inflater: Decompress::new(false),
            adler: (1, 0),
        }
    }
}

impl Inflate {
    fn adler(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.adler.0 = (self.adler.0 + u32::from(b)) % 65521;
            self.adler.1 = (self.adler.1 + self.adler.0) % 65521;
        }
    }

    fn push(&mut self, input: &[u8]) -> Result<Vec<u8>, DataError> {
        self.buffer.extend_from_slice(input);
        let mut out = Vec::new();
        loop {
            match self.state {
                ZState::First => match self.buffer.first() {
                    None => break,
                    Some(first) => {
                        self.raw = first & 0x0f != 0x08;
                        self.state = if self.raw {
                            ZState::Body
                        } else {
                            ZState::Header
                        };
                    }
                },
                ZState::Header => {
                    if self.buffer.len() < 2 {
                        break;
                    }
                    let (cmf, flg) = (self.buffer[0], self.buffer[1]);
                    // incorrect header check, invalid window size, need dictionary
                    if (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0
                        || cmf >> 4 > 7
                        || flg & 0x20 != 0
                    {
                        return Err(DataError);
                    }
                    self.buffer.drain(..2);
                    self.state = ZState::Body;
                }
                ZState::Body => {
                    let (taken, decoded, ended) = inflate(&mut self.inflater, &self.buffer)?;
                    self.buffer.drain(..taken);
                    self.adler(&decoded);
                    out.extend_from_slice(&decoded);
                    if !ended {
                        break;
                    }
                    self.state = if self.raw {
                        ZState::Done
                    } else {
                        ZState::Trailer
                    };
                }
                ZState::Trailer => {
                    if self.buffer.len() < 4 {
                        break;
                    }
                    let sum = u32::from_be_bytes(self.buffer[..4].try_into().expect("4 bytes"));
                    if sum != self.adler.1 << 16 | self.adler.0 {
                        return Err(DataError); // incorrect data check
                    }
                    self.buffer.drain(..4);
                    self.state = ZState::Done;
                }
                // What follows the stream is not read.
                ZState::Done => {
                    self.buffer.clear();
                    break;
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn gzip(text: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(text).unwrap();
        encoder.finish().unwrap()
    }

    fn decode(encoding: &str, body: &[u8], piece: usize) -> Result<Vec<u8>, DataError> {
        let mut decoders = Decoders::new(Some(encoding), 200);
        let mut out = Vec::new();
        for chunk in body.chunks(piece.max(1)) {
            out.extend(decoders.push(chunk)?);
        }
        Ok(out)
    }

    #[test]
    fn gzip_ends_as_nodes_fetch_reads_them() {
        let text =
            b"data: {\"choices\":[{\"delta\":{\"content\":\"compressed stream\"}}]}\n\n".repeat(50);
        let full = gzip(&text);
        for piece in [1, 7, full.len()] {
            assert_eq!(decode("gzip", &full, piece).unwrap(), text);
            assert_eq!(
                decode("gzip", &full[..full.len() - 8], piece).unwrap(),
                text,
                "no trailer"
            );
            assert_eq!(
                decode("gzip", &full[..full.len() - 4], piece).unwrap(),
                text,
                "half a trailer"
            );
            assert_eq!(
                decode("gzip", &full[..10], piece).unwrap(),
                b"",
                "a bare header"
            );
            assert_eq!(
                decode("gzip", &[full.clone(), full.clone()].concat(), piece).unwrap(),
                [text.clone(), text.clone()].concat()
            );
            let mut bad = full.clone();
            let at = bad.len() - 8;
            bad[at] ^= 1;
            assert_eq!(decode("gzip", &bad, piece), Err(DataError), "a wrong CRC");
            assert_eq!(decode("gzip", b"plain text", piece), Err(DataError));
        }
        assert_eq!(
            decode(
                "gzip",
                &[full.clone(), b"xyz".to_vec()].concat(),
                full.len() + 3
            ),
            Err(DataError)
        );
        assert_eq!(
            decode(
                "gzip",
                &[full.clone(), vec![0, 0, 1]].concat(),
                full.len() + 3
            )
            .unwrap(),
            text
        );
        // An unknown coding, identity among them, is read as it is.
        assert_eq!(decode("identity", b"raw", 3).unwrap(), b"raw");
        assert_eq!(decode("gzip, br", b"raw", 3).unwrap(), b"raw");
    }
}
