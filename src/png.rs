//! 极简 PNG 编码器（RGB24、zlib stored blocks），零外部依赖。
//!
//! 用途：剪贴板通道捕获后要写 `clip-*.png` 到收件箱（等价 nativeImage.toPNG()）。
//! 这里不实现 deflate 压缩，只写 stored blocks —— 输出是合法 PNG，可被任何解码器读取，
//! 但文件体积明显大于 Chromium 压缩过的 PNG。这是已知的、需要确认的兼容差异，
//! 见 ACCEPTANCE.md 边界 B9；它不影响内存指标，也不影响收件箱文件格式。

use std::io::Write;

pub fn encode_rows(width: u32, height: u32, rows: &[u8]) -> Vec<u8> {
    let row_len = 1 + width as usize * 3;
    if rows.len() < row_len * height as usize {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(rows.len() + 256);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8bit / truecolor / deflate / filter / no interlace
    chunk(&mut out, b"IHDR", &ihdr);

    // zlib: CMF=0x78 FLG=0x01 -> stored blocks, adler32 校验
    let raw = &rows[..row_len * height as usize];
    let mut z = Vec::with_capacity(raw.len() + raw.len() / 65535 * 5 + 16);
    z.extend_from_slice(&[0x78, 0x01]);
    let mut pos = 0usize;
    while pos < raw.len() {
        let n = (raw.len() - pos).min(65535);
        let final_block = pos + n >= raw.len();
        z.push(if final_block { 1 } else { 0 });
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&raw[pos..pos + n]);
        pos += n;
    }
    let mut a: u32 = 1;
    let mut bsum: u32 = 0;
    for &byte in raw {
        a = (a + byte as u32) % 65521;
        bsum = (bsum + a) % 65521;
    }
    z.extend_from_slice(&((bsum << 16) | a).to_be_bytes());
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    let _ = out.flush();
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_data = Vec::with_capacity(4 + data.len());
    crc_data.extend_from_slice(kind);
    crc_data.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &byte in data {
        c = table[((c ^ byte as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_a_structurally_valid_png() {
        // 2x2 RGB，每行前导 filter byte = 0
        let w = 2u32;
        let h = 2u32;
        let mut rows = vec![0u8];
        rows.extend_from_slice(&[255, 0, 0, 0, 255, 0]);
        rows.push(0);
        rows.extend_from_slice(&[0, 0, 255, 255, 255, 255]);
        let png = encode_rows(w, h, &rows);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), w);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), h);
        assert_eq!(png[24], 8);
        assert_eq!(png[25], 2);
        assert!(png.windows(4).any(|v| v == b"IDAT"));
        assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
    }

    #[test]
    fn crc_known_vector() {
        // CRC-32/ISO-HDLC 标准检查值
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn short_rows_return_empty() {
        assert!(encode_rows(100, 100, &[0u8; 4]).is_empty());
    }
}
