//! A minimal PNG writer.
//!
//! Exists so cover art can be handed to ffmpeg for video export without
//! depending on how the user's ffmpeg was built. Distro and static ffmpeg
//! builds very often lack librsvg, and "video export works on the developer's
//! machine" is not a feature.
//!
//! Deliberately no compression: PNG's zlib stream permits *stored* (literal)
//! blocks, which every decoder must support, and a cover is written once and
//! read once by a subprocess. Trading a megabyte of temporary file for a deflate
//! implementation — or a dependency — is the right way round here.

/// Encode 8-bit RGB into a PNG. `rgb` must be `width * height * 3` bytes.
pub fn encode_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    assert_eq!(
        rgb.len(),
        width as usize * height as usize * 3,
        "rgb size mismatch"
    );

    let mut out = Vec::with_capacity(rgb.len() + 4096);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, truecolour, no interlace
    chunk(&mut out, b"IHDR", &ihdr);

    // Every scanline carries a filter byte; 0 means "stored as-is".
    let mut raw = Vec::with_capacity(rgb.len() + height as usize);
    for row in rgb.chunks_exact(width as usize * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc = Crc::new();
    crc.push(kind);
    crc.push(body);
    out.extend_from_slice(&crc.finish().to_be_bytes());
}

/// A zlib stream made entirely of uncompressed deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    // 0x78 0x01: deflate, 32K window, no preset dictionary, fastest level.
    let mut out = vec![0x78, 0x01];
    // A stored block's length field is 16 bits, so the payload is chunked.
    const MAX: usize = 65_535;
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    for (i, block) in data.chunks(MAX).enumerate() {
        let last = (i + 1) * MAX >= data.len();
        out.push(if last { 1 } else { 0 });
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

struct Crc(u32);

impl Crc {
    fn new() -> Self {
        Crc(0xffff_ffff)
    }
    fn push(&mut self, bytes: &[u8]) {
        for &b in bytes {
            let mut c = (self.0 ^ b as u32) & 0xff;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            self.0 = c ^ (self.0 >> 8);
        }
    }
    fn finish(self) -> u32 {
        self.0 ^ 0xffff_ffff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_png_a_decoder_would_accept() {
        let rgb = vec![7u8; 4 * 3 * 3];
        let png = encode_rgb(4, 3, &rgb);

        assert_eq!(
            &png[0..8],
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
        // IHDR length, then the type, then the dimensions we asked for.
        assert_eq!(&png[8..12], &13u32.to_be_bytes());
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &4u32.to_be_bytes());
        assert_eq!(&png[20..24], &3u32.to_be_bytes());
        assert_eq!(png[24], 8, "bit depth");
        assert_eq!(png[25], 2, "truecolour");
        assert!(png.ends_with(&[b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]));
    }

    #[test]
    fn checksums_match_the_reference_values() {
        // The CRC of the empty IEND chunk is a fixed, well-known constant, so a
        // wrong table or a wrong bit order shows up immediately.
        let mut crc = Crc::new();
        crc.push(b"IEND");
        crc.push(&[]);
        assert_eq!(crc.finish(), 0xae42_6082);

        // Adler-32 of "abc", from the zlib spec's own worked example.
        assert_eq!(adler32(b"abc"), 0x024d_0127);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn payloads_larger_than_one_stored_block_still_terminate() {
        // Two full blocks plus a remainder: only the last may set the final bit.
        let data = vec![0u8; 65_535 * 2 + 10];
        let z = zlib_stored(&data);
        assert_eq!(&z[0..2], &[0x78, 0x01]);

        let mut pos = 2;
        let mut blocks = 0;
        let mut finals = 0;
        loop {
            let last = z[pos] & 1;
            let len = u16::from_le_bytes([z[pos + 1], z[pos + 2]]) as usize;
            let nlen = u16::from_le_bytes([z[pos + 3], z[pos + 4]]);
            assert_eq!(nlen, !(len as u16), "stored block length check");
            blocks += 1;
            finals += last as usize;
            pos += 5 + len;
            if last == 1 {
                break;
            }
        }
        assert_eq!(blocks, 3);
        assert_eq!(finals, 1);
        // Four bytes of Adler-32 close the stream.
        assert_eq!(pos + 4, z.len());
    }
}
