//! 视觉哈希：24x24 缩放 -> RGB 各量化到 8 级 -> SHA1。
//!
//! 等价目标：lib/capture.js 的 visualHash() / visualKey()
//!   small = img.resize({width:24,height:24}); bmp = small.toBitmap()  // BGRA
//!   buf[i*3  ] = bmp[i*4+2] >> 5   // R
//!   buf[i*3+1] = bmp[i*4+1] >> 5   // G
//!   buf[i*3+2] = bmp[i*4  ] >> 5   // B
//!   key = sha1(buf) + '|' + width + 'x' + height
//!
//! 说明：哈希只用于"同一通道内 / 同一实现内"判重；跨通道判重靠 尺寸+10s 时间窗，
//! 因此缩略图插值算法与 Chromium 不同不会破坏行为等价（见 ACCEPTANCE.md 边界 B7）。

pub const THUMB: usize = 24;
pub const PIXELS: usize = THUMB * THUMB;

/// 标准 SHA1（无外部 crate）。返回 40 字符小写十六进制。
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity((data.len() + 72) & !63);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut w = [0u32; 80];
    for chunk in msg.chunks_exact(64) {
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = String::with_capacity(40);
    for x in h {
        out.push_str(&format!("{:08x}", x));
    }
    out
}

/// BGRA 像素（4 字节/像素，长度 >= PIXELS*4）-> 1728 字节量化缓冲
pub fn quantize_bgra(bgra: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(PIXELS * 3);
    for i in 0..PIXELS {
        let o = i * 4;
        if o + 3 >= bgra.len() {
            break;
        }
        buf.push(bgra[o + 2] >> 5); // R
        buf.push(bgra[o + 1] >> 5); // G
        buf.push(bgra[o] >> 5); // B
    }
    buf
}

/// 量化缓冲 -> `${sha1}|${w}x${h}`；等价于 visualKey() 的 key 字段
pub fn visual_key(quantized: &[u8], width: u32, height: u32) -> String {
    format!("{}|{}x{}", sha1_hex(quantized), width, height)
}

/// 尺寸串，等价 visualKey() 的 dims 字段
pub fn dims(width: u32, height: u32) -> String {
    format!("{}x{}", width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_known_vectors() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            sha1_hex(b"The quick brown fox jumps over the lazy dog"),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        );
    }

    #[test]
    fn quantize_matches_js_channel_order() {
        // BGRA: B=224(111), G=31(000), R=255(111), A=255
        let mut px = vec![0u8; PIXELS * 4];
        px[0] = 224;
        px[1] = 31;
        px[2] = 255;
        px[3] = 255;
        let q = quantize_bgra(&px);
        assert_eq!(&q[0..3], &[7, 0, 7]);
        assert_eq!(q.len(), PIXELS * 3);
    }

    #[test]
    fn key_format_matches_js() {
        let q = vec![0u8; PIXELS * 3];
        let k = visual_key(&q, 1920, 1080);
        assert!(k.ends_with("|1920x1080"));
        assert_eq!(k.len(), 40 + 1 + 9);
    }
}
