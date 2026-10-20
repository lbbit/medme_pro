// 极简 base64 解码器（RFC 4648 标准字母表 + '=' 填充）。
//
// 为什么手写而不引 base64 crate：本项目最值钱的不变量之一是「依赖面最小 + 构建期稳定」
// （D-017 选型理由同形：能自己钉死并被测试锁死的东西就别交给外部 crate）。
// 写侧工具需要在 JSON 里传二进制原件 —— 二进制进 JSON 的通用做法就是 base64。
//
// 语义：
//   · 忽略空白（\n \r 空格 \t）——LLM 常把长串折行；
//   · '=' 只允许出现在末尾（最多 2 个），且长度必须自洽（4 的倍数）；
//   · 非法字符 / 长度不自洽 ⇒ Err(说明)，**绝不返回"部分结果"**（D5：不猜、不错报）。
use std::fmt;

#[derive(Debug)]
pub struct DecodeError(pub String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid base64: {}", self.0)
    }
}

fn val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// 解码标准 base64 字符串为字节。
pub fn decode(s: &str) -> Result<Vec<u8>, DecodeError> {
    // 1) 清洗：去掉空白（LLM 常折行），收集数据字符与尾部 padding。
    let mut syms: Vec<u8> = Vec::with_capacity(s.len());
    let mut padding = 0usize;
    let mut seen_pad = false;
    for &b in s.as_bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            seen_pad = true;
            padding += 1;
            if padding > 2 {
                return Err(DecodeError("too much padding".into()));
            }
            continue;
        }
        if seen_pad {
            return Err(DecodeError("data character after padding".into()));
        }
        let v = val(b).ok_or_else(|| DecodeError(format!("illegal character {:?}", b as char)))?;
        syms.push(v);
    }

    // 2) 长度自洽：总字符数 = 4k，且末组只可能是 2/3 字符（+2/1 padding）或 4 字符（+0 padding）。
    if syms.len() % 4 == 1 {
        return Err(DecodeError("truncated quantum (1 char left)".into()));
    }
    let rem = syms.len() % 4;
    let expect_pad = match rem {
        0 => 0,
        2 => 2,
        3 => 1,
        _ => unreachable!(),
    };
    if padding != expect_pad {
        return Err(DecodeError(format!(
            "padding mismatch: {padding} '=' but {rem} leftover chars need {expect_pad}"
        )));
    }

    // 3) 解码：每 4 个 sextet → 3 字节；末组按 rem 截掉多余字节（并校验被丢弃的位为 0，严格模式）。
    let mut acc: Vec<u8> = Vec::with_capacity(syms.len() / 4 * 3);
    let mut last_n: u32 = 0;
    for chunk in syms.chunks(4) {
        let n = (chunk[0] as u32) << 18
            | (chunk[1] as u32) << 12
            | (*chunk.get(2).unwrap_or(&0) as u32) << 6
            | (*chunk.get(3).unwrap_or(&0) as u32);
        last_n = n;
        acc.push((n >> 16) as u8);
        acc.push((n >> 8) as u8);
        acc.push(n as u8);
    }
    match rem {
        2 => {
            // 1 字节有效：末组 12 位中低 4 位必须为 0（严格模式，防「一串多解」）
            if last_n & 0xFFFF != 0 {
                return Err(DecodeError("non-zero discarded bits".into()));
            }
            acc.truncate(acc.len() - 2);
        }
        3 => {
            // 2 字节有效：末组 18 位中低 2 位必须为 0
            if last_n & 0x3F != 0 {
                return Err(DecodeError("non-zero discarded bits".into()));
            }
            acc.truncate(acc.len() - 1);
        }
        _ => {}
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_known_vectors() {
        // RFC 4648 §10 测试向量
        assert_eq!(decode("").unwrap(), b"");
        assert_eq!(decode("Zg==").unwrap(), b"f");
        assert_eq!(decode("Zm8=").unwrap(), b"fo");
        assert_eq!(decode("Zm9v").unwrap(), b"foo");
        assert_eq!(decode("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(decode("Zm9vYmE=").unwrap(), b"fooba");
        assert_eq!(decode("Zm9vYmFy").unwrap(), b"foobar");
    }

    #[test]
    fn ignores_whitespace() {
        assert_eq!(decode("Zm9v\nYmFy").unwrap(), b"foobar");
        assert_eq!(decode(" Zm9v YmFy ").unwrap(), b"foobar");
    }

    #[test]
    fn binary_roundtrip() {
        let data: Vec<u8> = (0u8..=255).collect();
        // 用内置编码器做往返（测试侧允许写一个临时编码器，生产代码只需解码）
        let enc = |d: &[u8]| -> String {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut out = String::new();
            for chunk in d.chunks(3) {
                let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
                let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
                out.push(T[(n >> 18) as usize & 63] as char);
                out.push(T[(n >> 12) as usize & 63] as char);
                out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
                out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
            }
            out
        };
        assert_eq!(decode(&enc(&data)).unwrap(), data);
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode("****").is_err());
        assert!(decode("Zg=A").is_err(), "data after padding must be rejected");
        assert!(decode("Zg===").is_err());
        assert!(decode("A").is_err(), "truncated quantum");
        assert!(decode("Zg=").is_err(), "padding mismatch");
        assert!(decode("Zg").is_err(), "missing padding");
        assert!(decode("Zm9vYg==").is_ok());
        assert!(decode("Zm9vYmFy=").is_err(), "padding on complete quantum");
    }

    /// 严格模式：末组被丢弃的位必须为 0（否则同一串可对应多个字节序列）。
    #[test]
    fn rejects_non_zero_discarded_bits() {
        // "Zh==" 解出 'f' + 丢弃位非零 ⇒ 拒
        assert!(decode("Zh==").is_err());
        assert!(decode("Zm9vYg=").is_err());
        assert!(decode("Zg==").is_ok());
    }
}
