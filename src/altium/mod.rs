//! 将统一器件模型写为 Altium ASCII 原理图与 PCB 文档。 / Write unified component models as Altium ASCII schematic and PCB documents.
mod pcb;
mod sch;

use crate::model::{Component, Point, Segment};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

pub struct Documents {
    pub schematic: Vec<u8>,
    pub pcb: Vec<u8>,
}

/// 校验器件模型并生成配套的原理图与 PCB 文档字节。 / Validate the component model and render paired schematic and PCB document bytes.
pub fn render(component: &Component) -> Result<Documents> {
    component.check()?;
    Ok(Documents {
        schematic: sch::render(component)?,
        pcb: pcb::render(component)?,
    })
}

/// 从种子摘要派生可复现的八字母标识符。 / Derive a reproducible eight-letter identifier from the seed digest.
pub(crate) fn unique_id(seed: &str) -> String {
    Sha256::digest(seed.as_bytes())
        .iter()
        .take(8)
        .map(|b| char::from(b'A' + b % 26))
        .collect()
}

/// 保留至多六位小数，移除尾零并将负零规范为零。 / Keep at most six decimal places, trim trailing zeros, and normalize negative zero.
pub(crate) fn decimal(n: f64) -> String {
    let s = format!("{n:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// 以规范化十进制数和 mil 后缀输出长度。 / Format a length as a normalized decimal followed by the mil suffix.
pub(crate) fn mil(n: f64) -> String {
    format!("{}mil", decimal(n))
}

#[derive(Default)]
pub(crate) struct Record {
    fields: Vec<(String, String)>,
}
impl Record {
    /// 创建带指定 RECORD 类型字段的记录构造器。 / Create a record builder containing the specified RECORD type.
    pub fn new(kind: impl ToString) -> Self {
        Self::default().set("RECORD", kind)
    }

    /// 按调用顺序追加键值字段，并返回构造器用于链式调用。 / Append a field in call order and return the builder for chaining.
    pub fn set(mut self, key: impl Into<String>, value: impl ToString) -> Self {
        self.fields.push((key.into(), value.to_string()));
        self
    }

    /// 按十万分之一精度拆分坐标的整数与小数字段。 / Split a coordinate into integer and fractional fields at one hundred-thousandth precision.
    pub fn coord(mut self, key: &str, value: f64) -> Self {
        // 负坐标采用欧几里得拆分，确保小数字段保持非负。 / Use Euclidean decomposition for negative coordinates to keep fractional fields nonnegative.
        let raw = (value * 100_000.0).round() as i64;
        self = self.set(key, raw.div_euclid(100_000));
        if raw.rem_euclid(100_000) != 0 {
            self = self.set(format!("{key}_FRAC"), raw.rem_euclid(100_000));
        }
        self
    }

    /// 将二维点写入 LOCATION.X 与 LOCATION.Y 坐标字段。 / Write a 2D point into the LOCATION.X and LOCATION.Y fields.
    pub fn at(self, point: Point) -> Self {
        self.coord("LOCATION.X", point.x)
            .coord("LOCATION.Y", point.y)
    }

    /// 校验字段分隔符并编码单条记录，使用 CRLF 结束。 / Validate field delimiters and encode one record terminated with CRLF.
    pub fn line(self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        for (key, value) in self.fields {
            ensure!(
                !value.contains(['|', '\r', '\n']),
                "字段 {key} 包含不支持的分隔符"
            );
            bytes.extend_from_slice(format!("|{key}=").as_bytes());
            if value.is_ascii() {
                bytes.extend_from_slice(value.as_bytes());
            } else {
                // 为非 ASCII 内容同时保留兼容值和 UTF-8 扩展值。 / Preserve both a compatibility value and a UTF-8 extension for non-ASCII content.
                bytes.extend(
                    value
                        .chars()
                        .map(|c| if c.is_ascii() { c as u8 } else { b'?' }),
                );
                bytes.extend_from_slice(format!("|%UTF8%{key}={value}").as_bytes());
            }
        }
        bytes.extend_from_slice(b"\r\n");
        Ok(bytes)
    }
}

/// 提取圆弧的圆心、半径与规范化角度，直线返回空值。 / Extract an arc center, radius, and normalized angles, returning none for a line.
pub(crate) fn arc_angles(segment: &Segment) -> Option<(Point, f64, f64, f64)> {
    if let Segment::Arc {
        start,
        end,
        center,
        sweep,
    } = *segment
    {
        let mut a = (start.y - center.y)
            .atan2(start.x - center.x)
            .to_degrees()
            .rem_euclid(360.0);
        let mut b = (end.y - center.y)
            .atan2(end.x - center.x)
            .to_degrees()
            .rem_euclid(360.0);
        if sweep < 0.0 {
            std::mem::swap(&mut a, &mut b);
        }
        Some((center, (start.x - center.x).hypot(start.y - center.y), a, b))
    } else {
        None
    }
}

pub const SOURCE: &str = "嘉立创EDA/EasyEDA官方库";
pub const SOURCE_URL: &str = "https://lceda.cn/";
pub const EASYEDA_URL: &str = "https://easyeda.com/";
