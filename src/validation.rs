//! 校验 ASCII 记录结构、导出完整性及跨文档编号一致性。 / Validate ASCII record structure, export completeness, and cross-document identifier consistency.
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub type Properties = BTreeMap<String, String>;
/// 解析文档记录，拒绝重复字段并优先采用 UTF-8 扩展值。 / Parse document records, reject duplicate fields, and prefer UTF-8 extension values.
pub fn records(bytes: &[u8]) -> Result<Vec<Properties>> {
    let text = std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
        .context("Doc 文件不是可读取的 ASCII/UTF-8 文档")?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            ensure!(line.starts_with('|'), "Doc 记录没有正确的起始标识");
            let mut record = Properties::new();
            for field in line.split('|').skip(1) {
                if field.is_empty() {
                    continue;
                }
                let (key, value) = field.split_once('=').context("Doc 字段没有等号")?;
                ensure!(!record.contains_key(key), "Doc 存在重复字段 {key}");
                record.insert(key.to_owned(), value.to_owned());
            }
            // 用 UTF-8 扩展覆盖兼容值，恢复非 ASCII 字符的原文。 / Override compatibility values with UTF-8 extensions to restore the original non-ASCII text.
            let utf8: Vec<_> = record
                .iter()
                .filter_map(|(key, value)| {
                    key.strip_prefix("%UTF8%")
                        .map(|k| (k.to_owned(), value.clone()))
                })
                .collect();
            for (key, value) in utf8 {
                record.insert(key, value);
            }
            Ok(record)
        })
        .collect()
}

/// 读取记录中的字符串属性，属性缺失时返回空串。 / Read a string property from a record, returning an empty string when absent.
fn value<'a>(r: &'a Properties, key: &str) -> &'a str {
    r.get(key).map(String::as_str).unwrap_or_default()
}

/// 将必需属性解析为无符号计数，失败时标明属性名。 / Parse a required property as an unsigned count, naming the property on failure.
fn count(r: &Properties, key: &str) -> Result<usize> {
    value(r, key)
        .parse()
        .with_context(|| format!("缺少有效的 {key}"))
}

/// 校验尺寸字段为有限正数，并允许 mil 单位后缀。 / Validate a dimension as finite and positive, allowing a mil suffix.
fn positive(r: &Properties, key: &str) -> Result<()> {
    let n: f64 = value(r, key)
        .trim_end_matches("mil")
        .parse()
        .with_context(|| format!("无效尺寸 {key}"))?;
    ensure!(n.is_finite() && n > 0.0, "尺寸 {key} 必须大于零");
    Ok(())
}

/// 解析并校验有限数值，允许 mil 单位后缀。 / Parse and validate a finite number, allowing a mil suffix.
fn finite(r: &Properties, key: &str) -> Result<f64> {
    let n: f64 = value(r, key)
        .trim_end_matches("mil")
        .parse()
        .with_context(|| format!("无效数值 {key}"))?;
    ensure!(n.is_finite(), "字段 {key} 不是有限数值");
    Ok(n)
}

/// 核对两份文档的结构、器件身份、记录数量及引脚与焊盘对应关系。 / Verify both documents for structure, component identity, record counts, and pin-to-pad mappings.
pub fn validate_documents(schematic: &[u8], pcb: &[u8], model: &str, id: &str) -> Result<()> {
    let sch = records(schematic)?;
    let pcb = records(pcb)?;
    ensure!(
        sch.len() > 3
            && value(&sch[0], "HEADER")
                == "Protel for Windows - Schematic Capture Ascii File Version 5.0",
        "SchDoc 格式标识错误"
    );
    ensure!(
        value(sch.last().unwrap(), "HEADER") == "Icon storage",
        "SchDoc 结尾缺失或文件被截断"
    );
    let sch = &sch[1..sch.len() - 1];
    let header = records(schematic)?.remove(0);
    ensure!(
        count(&header, "WEIGHT")? == sch.len(),
        "SchDoc 记录数量不完整"
    );
    ensure!(value(&sch[0], "RECORD") == "31", "SchDoc 没有图纸配置");
    let components: Vec<_> = sch
        .iter()
        .enumerate()
        .filter(|(_, r)| value(r, "RECORD") == "1")
        .collect();
    ensure!(!components.is_empty(), "SchDoc 没有元件");
    let mut parts = BTreeSet::new();
    for (_, component) in &components {
        ensure!(
            value(component, "LIBREFERENCE") == model,
            "SchDoc 型号不匹配"
        );
        parts.insert(count(component, "CURRENTPARTID")?);
        ensure!(
            count(component, "PARTCOUNT")? == components.len() + 1,
            "SchDoc 多单元数量不完整"
        );
    }
    ensure!(parts.len() == components.len(), "SchDoc 多单元编号重复");
    for index in 1..=parts.len() {
        ensure!(parts.contains(&index), "SchDoc 缺少子单元 {index}");
    }
    let pins: Vec<_> = sch.iter().filter(|r| value(r, "RECORD") == "2").collect();
    ensure!(!pins.is_empty(), "SchDoc 没有引脚");
    for pin in &pins {
        let owner = count(pin, "OWNERINDEX")?;
        let component = sch.get(owner).context("引脚父对象索引越界")?;
        ensure!(
            value(component, "RECORD") == "1"
                && value(pin, "OWNERPARTID") == value(component, "CURRENTPARTID"),
            "引脚所属单元不匹配"
        );
        ensure!(!value(pin, "DESIGNATOR").is_empty(), "引脚没有编号");
        positive(pin, "PINLENGTH")?;
        finite(pin, "LOCATION.X")?;
        finite(pin, "LOCATION.Y")?;
        ensure!(count(pin, "ELECTRICAL")? <= 7, "引脚电气类型无效");
        count(pin, "PINCONGLOMERATE")?;
    }
    let expected_pins = sch
        .iter()
        .find(|r| value(r, "NAME") == "LCSC_SOURCE_PIN_COUNT")
        .context("缺少源引脚统计")?;
    ensure!(
        count(expected_pins, "TEXT")? == pins.len(),
        "SchDoc 引脚丢失"
    );
    ensure!(
        sch.iter()
            .any(|r| value(r, "NAME") == "LCSC_ID" && value(r, "TEXT") == id),
        "SchDoc 器件编号不匹配"
    );
    ensure!(
        pcb.len() > 3
            && value(&pcb[0], "RECORD") == "Board"
            && value(&pcb[0], "KIND") == "Protel_Advanced_PCB",
        "PcbDoc 格式标识错误"
    );
    let footer = pcb.last().unwrap();
    ensure!(
        value(footer, "LCSC_EXPORT_COMPLETE") == "TRUE" && value(footer, "LCSC_ID") == id,
        "PcbDoc 结尾缺失或编号错误"
    );
    ensure!(
        count(footer, "LCSC_EXPORT_RECORD_COUNT")? == pcb.len(),
        "PcbDoc 图元记录丢失"
    );
    let component = pcb
        .iter()
        .find(|r| value(r, "RECORD") == "Component")
        .context("PcbDoc 没有封装");
    let component = component?;
    ensure!(
        value(component, "PATTERN") == model && value(component, "LCSC_ID") == id,
        "PcbDoc 型号或编号不匹配"
    );
    let pads: Vec<_> = pcb.iter().filter(|r| value(r, "RECORD") == "Pad").collect();
    ensure!(
        !pads.is_empty() && pads.len() == count(footer, "LCSC_SOURCE_PAD_COUNT")?,
        "PcbDoc 焊盘丢失"
    );
    ensure!(
        pins.len() == count(footer, "LCSC_SOURCE_PIN_COUNT")?,
        "两个文档的源引脚统计不一致"
    );
    let expected_pads = sch
        .iter()
        .find(|r| value(r, "NAME") == "LCSC_SOURCE_PAD_COUNT")
        .context("缺少源焊盘统计")?;
    ensure!(
        pads.len() == count(expected_pads, "TEXT")?,
        "两个文档的源焊盘统计不一致"
    );
    for pad in &pads {
        ensure!(
            value(pad, "COMPONENT") == value(component, "ID"),
            "焊盘没有关联到封装"
        );
        positive(pad, "XSIZE")?;
        positive(pad, "YSIZE")?;
        finite(pad, "X")?;
        finite(pad, "Y")?;
        finite(pad, "ROTATION")?;
        ensure!(finite(pad, "HOLESIZE")? >= 0.0, "钻孔尺寸无效");
    }
    let pin_ids: BTreeSet<_> = pins.iter().map(|p| value(p, "DESIGNATOR")).collect();
    let pad_ids: BTreeSet<_> = pads
        .iter()
        .map(|p| value(p, "NAME"))
        .filter(|v| !v.is_empty())
        .collect();
    ensure!(pin_ids == pad_ids, "两个文档的引脚与焊盘编号不对应");
    Ok(())
}

/// 要求目录恰好包含两份指定 Doc 文件，且全部通过文档校验。 / Require exactly the two named Doc files in the directory and validate both documents.
pub fn validate_complete(directory: &Path, name: &str, id: &str) -> Result<()> {
    let sch = std::fs::read(directory.join(format!("{name}.SchDoc")))?;
    let pcb = std::fs::read(directory.join(format!("{name}.PcbDoc")))?;
    validate_documents(&sch, &pcb, name, id)?;
    let files: Vec<_> = std::fs::read_dir(directory)?.collect::<std::io::Result<_>>()?;
    ensure!(
        files.len() == 2 && files.iter().all(|entry| entry.path().is_file()),
        "输出目录不恰好包含两个 Doc 文件"
    );
    Ok(())
}
