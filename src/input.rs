use std::{collections::HashSet, fmt::Write};

use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

/// 从文本提取 C 编号，统一大写并按首次出现顺序去重。 / Extract C IDs, normalize them to uppercase, and deduplicate in first-seen order.
pub fn parse_ids(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| {
            token.len() > 1
                && token.starts_with(['C', 'c'])
                && token.as_bytes()[1..].iter().all(u8::is_ascii_digit)
        })
        .map(str::to_ascii_uppercase)
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// 清理非法文件名字符及保留名，使用摘要缩短过长型号。 / Sanitize invalid filename characters and reserved names, shortening long models with a digest.
pub fn safe_name(model: &str) -> Result<String> {
    let name: String = model
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut name = name.trim().trim_matches(['.', ' ']).to_owned();
    ensure!(!name.is_empty(), "型号为空，无法创建文件名");
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
    {
        name.insert(0, '_');
    }
    if name.chars().count() > 120 {
        // 保留摘要前六字节的补零小写十六进制，兼容已有长文件名。 / Keep zero-padded lowercase hex for the first six digest bytes to preserve existing long filenames.
        let digest = Sha256::digest(name.as_bytes());
        let mut suffix = String::with_capacity(12);
        for byte in &digest[..6] {
            write!(&mut suffix, "{byte:02x}")?;
        }
        name = format!("{}_{}", name.chars().take(100).collect::<String>(), suffix);
    }
    Ok(name)
}

/// 优先解码 UTF-8 或带字节序标记的 UTF-16，否则回退到 GBK。 / Decode UTF-8 or BOM-marked UTF-16 first, falling back to GBK otherwise.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return encoding_rs::UTF_16LE.decode(bytes).0.into_owned();
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return encoding_rs::UTF_16BE.decode(bytes).0.into_owned();
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .unwrap_or_else(|_| encoding_rs::GBK.decode(bytes).0.into_owned())
}
