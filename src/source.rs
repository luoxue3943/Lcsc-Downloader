//! 解析专业版符号与封装数组行格式，遇到不兼容格式即终止转换。 / Parse Pro symbol and footprint array-line formats, stopping conversion for incompatible formats.
use crate::model::*;
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::collections::HashMap;

/// 读取数组或逐行 JSON 数据，并校验资源类型与版本。 / Read arrays or line-delimited JSON and validate the resource type and version.
pub fn rows(payload: &Value, kind: &str) -> Result<Vec<Vec<Value>>> {
    let data = payload
        .get("result")
        .unwrap_or(payload)
        .get("dataStr")
        .context("接口未返回 dataStr")?;
    let parsed: Vec<Vec<Value>> = if let Some(text) = data.as_str() {
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str(line).context("不支持的源文件格式，需要 JSON 数组行格式")
            })
            .collect::<Result<_>>()?
    } else if data.is_array() {
        serde_json::from_value(data.clone())?
    } else {
        bail!("当前仅支持专业版数组行格式，未使用近似转换替代其他格式");
    };
    let header = parsed.first().context("源文件为空")?;
    ensure!(
        text(header, 0) == "DOCTYPE"
            && text(header, 1) == kind
            && text(header, 2).starts_with("1."),
        "不支持的 {kind} 格式版本"
    );
    Ok(parsed)
}

/// 读取指定字符串字段，字段缺失或类型不符时返回空串。 / Read a string field, returning an empty string when missing or mistyped.
fn text(row: &[Value], index: usize) -> &str {
    row.get(index).and_then(Value::as_str).unwrap_or_default()
}

/// 读取必需的有限数值字段，错误中保留图元与字段位置。 / Read a required finite number, preserving the primitive and field position in errors.
fn num(row: &[Value], index: usize) -> Result<f64> {
    let value = row
        .get(index)
        .and_then(Value::as_f64)
        .with_context(|| format!("{} 的第 {index} 个字段不是数值", text(row, 0)))?;
    ensure!(value.is_finite(), "图元包含无效数值");
    Ok(value)
}

/// 读取可选数值字段，缺失或类型不符时使用默认值。 / Read an optional number, using the default when missing or mistyped.
fn optional_num(row: &[Value], index: usize, default: f64) -> f64 {
    row.get(index).and_then(Value::as_f64).unwrap_or(default)
}

/// 从两个必需数值字段读取并校验坐标点。 / Read and validate a point from two required numeric fields.
fn point(row: &[Value], x: usize, y: usize) -> Result<Point> {
    Point::new(num(row, x)?, num(row, y)?)
}

/// 按布尔值或非零数值解释开关字段。 / Interpret a flag as a boolean or a nonzero number.
fn flag(row: &[Value], index: usize) -> bool {
    row.get(index)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| optional_num(row, index, 0.0) != 0.0)
}

/// 将受支持的路径指令展开为直线与圆弧，遇到未知指令报错。 / Expand supported path commands into lines and arcs, rejecting unknown commands.
pub fn path(value: &Value) -> Result<Vec<Segment>> {
    let values = value.as_array().context("路径不是数组")?;
    if values.first().and_then(Value::as_str) == Some("CIRCLE") {
        let center = point(values, 1, 2)?;
        let r = num(values, 3)?;
        ensure!(r > 0.0, "圆的半径无效");
        let a = center.offset(r, 0.0);
        let b = center.offset(-r, 0.0);
        return Ok(vec![
            Segment::Arc {
                start: a,
                end: b,
                center,
                sweep: 180.0,
            },
            Segment::Arc {
                start: b,
                end: a,
                center,
                sweep: 180.0,
            },
        ]);
    }
    ensure!(values.len() >= 2, "路径没有起点");
    let mut current = point(values, 0, 1)?;
    let first = current;
    let mut index = 2;
    let mut result = Vec::new();
    while index < values.len() {
        if let Some(command) = values[index].as_str() {
            index += 1;
            match command.to_ascii_uppercase().as_str() {
                "L" => {}
                "ARC" => {
                    let sweep = num(values, index)?;
                    let end = point(values, index + 1, index + 2)?;
                    result.push(Segment::arc(current, end, sweep)?);
                    current = end;
                    index += 3;
                    continue;
                }
                "Z" => {
                    if (current.x - first.x).hypot(current.y - first.y) > 1e-8 {
                        result.push(Segment::Line {
                            start: current,
                            end: first,
                        });
                    }
                    current = first;
                    continue;
                }
                _ => bail!("尚不支持路径指令 {command}，已停止导出以避免丢失图形"),
            }
            if index == values.len() {
                bail!("路径指令没有坐标");
            }
            continue;
        }
        let end = point(values, index, index + 1)?;
        result.push(Segment::Line {
            start: current,
            end,
        });
        current = end;
        index += 2;
    }
    ensure!(!result.is_empty(), "路径没有线段");
    Ok(result)
}

/// 以两个对角点构造由四条线段组成的闭合矩形。 / Build a closed four-segment rectangle from opposite corners.
fn rectangle(a: Point, b: Point) -> Vec<Segment> {
    let points = [a, Point { x: b.x, y: a.y }, b, Point { x: a.x, y: b.y }, a];
    points
        .windows(2)
        .map(|p| Segment::Line {
            start: p[0],
            end: p[1],
        })
        .collect()
}

/// 将源引脚电气类型映射为目标枚举编号，拒绝未知类型。 / Map source pin electrical types to target enum values and reject unknown types.
fn electrical(value: &str) -> Result<u8> {
    Ok(match value.to_ascii_uppercase().as_str() {
        "IN" | "INPUT" => 0,
        "BI" | "BIDIRECTIONAL" | "I/O" => 1,
        "OUT" | "OUTPUT" => 2,
        "OC" | "OPEN COLLECTOR" => 3,
        "UNDEFINED" | "PASSIVE" | "PASS" | "" => 4,
        "TRI" | "TRISTATE" | "TRI-STATE" => 5,
        "OE" | "OPEN EMITTER" => 6,
        "POWER" | "PWR" => 7,
        other => bail!("尚不支持引脚电气类型 {other}"),
    })
}

/// 将源图层编号映射为目标图层名称，拒绝未支持图层。 / Map source layer codes to target layer names and reject unsupported layers.
fn layer(code: i64) -> Result<&'static str> {
    Ok(match code {
        1 => "TOP",
        2 => "BOTTOM",
        3 => "TOPOVERLAY",
        4 => "BOTTOMOVERLAY",
        5 => "TOPSOLDER",
        6 => "BOTTOMSOLDER",
        7 => "TOPPASTE",
        8 => "BOTTOMPASTE",
        12 => "MULTILAYER",
        13 => "MECHANICAL1",
        48 => "MECHANICAL2",
        49 => "MECHANICAL3",
        50 => "MECHANICAL4",
        51 => "MECHANICAL5",
        9 => "MECHANICAL6",
        10 => "MECHANICAL7",
        14 => "MECHANICAL8",
        11 => "MECHANICAL9",
        56 => "DRILLDRAWING",
        _ => bail!("尚不支持源图层 {code} 的转换"),
    })
}

/// 关联产品、符号和封装数据，构建并校验统一器件模型。 / Combine product, symbol, and footprint data into a validated component model.
pub fn convert(product: &Value, symbol: &Value, footprint: &Value) -> Result<Component> {
    let symbol = rows(symbol, "SYMBOL")?;
    let footprint = rows(footprint, "FOOTPRINT")?;
    let mut component = Component {
        id: product["id"].as_str().context("没有 C 编号")?.into(),
        model: product["model"].as_str().context("没有具体型号")?.into(),
        description: product["description"].as_str().unwrap_or_default().into(),
        designator: product["attributes"]["Designator"]
            .as_str()
            .unwrap_or("U?")
            .replace('?', "1"),
        source_license: String::new(),
        parts: Vec::new(),
        pads: Vec::new(),
        graphics: Vec::new(),
    };
    // 先索引分离存储的属性行，防止属性位置影响引脚元数据关联。 / Index separately stored attributes first so their positions do not affect pin metadata resolution.
    let mut attrs: HashMap<&str, HashMap<&str, &Vec<Value>>> = HashMap::new();
    for row in &symbol {
        if text(row, 0) == "ATTR" {
            attrs
                .entry(text(row, 2))
                .or_default()
                .insert(text(row, 3), row);
        }
        if text(row, 0) == "HEAD" {
            component.source_license = row
                .get(1)
                .and_then(|v| v["license"].as_str())
                .unwrap_or_default()
                .to_owned();
        }
    }
    for row in &symbol {
        let kind = text(row, 0);
        if kind == "PART" {
            component.parts.push(SymbolPart {
                name: text(row, 1).into(),
                pins: Vec::new(),
                graphics: Vec::new(),
            });
            continue;
        }
        if matches!(
            kind,
            "DOCTYPE" | "HEAD" | "LINESTYLE" | "FONTSTYLE" | "ATTR"
        ) {
            continue;
        }
        let part = component
            .parts
            .last_mut()
            .context("符号图元没有所属 PART")?;
        match kind {
            "PIN" => {
                let attr = attrs.get(text(row, 1));
                let get = |name| attr.and_then(|a| a.get(name)).copied();
                let number = get("NUMBER")
                    .map(|r| text(r, 4))
                    .filter(|v| !v.is_empty())
                    .context("引脚缺少编号")?;
                let angle = num(row, 7)?;
                ensure!(
                    (angle / 90.0 - (angle / 90.0).round()).abs() < 1e-6,
                    "仅支持正交引脚方向"
                );
                let length = num(row, 6)?;
                ensure!(length > 0.0, "引脚长度无效");
                let style = optional_num(row, 9, 0.0) as u32;
                part.pins.push(Pin {
                    number: number.into(),
                    name: get("NAME").map(|r| text(r, 4)).unwrap_or(number).into(),
                    hot: point(row, 4, 5)?,
                    length,
                    inward_angle: angle,
                    electrical: electrical(
                        get("Pin Type").map(|r| text(r, 4)).unwrap_or("Undefined"),
                    )?,
                    show_name: get("NAME").is_some_and(|r| flag(r, 6)),
                    show_number: get("NUMBER").is_some_and(|r| flag(r, 6)),
                    inverted: style & 2 != 0,
                    clock: style & 1 != 0,
                    hidden: !flag(row, 2),
                });
            }
            "RECT" => {
                ensure!(
                    optional_num(row, 6, 0.0) == 0.0
                        && optional_num(row, 7, 0.0) == 0.0
                        && optional_num(row, 8, 0.0) == 0.0,
                    "暂不支持圆角或旋转符号矩形"
                );
                part.graphics.push(Graphic {
                    shape: Shape::Path {
                        segments: rectangle(point(row, 2, 3)?, point(row, 4, 5)?),
                        filled: false,
                    },
                    layer: String::new(),
                    width: 1.0,
                });
            }
            "ELLIPSE" => {
                ensure!(optional_num(row, 6, 0.0) == 0.0, "不支持旋转椭圆");
                part.graphics.push(Graphic {
                    shape: Shape::Ellipse {
                        center: point(row, 2, 3)?,
                        rx: num(row, 4)?,
                        ry: num(row, 5)?,
                        filled: false,
                    },
                    layer: String::new(),
                    width: 1.0,
                });
            }
            "POLY" => {
                part.graphics.push(Graphic {
                    shape: Shape::Path {
                        segments: path(row.get(2).context("POLY 没有路径")?)?,
                        filled: false,
                    },
                    layer: String::new(),
                    width: 1.0,
                });
            }
            "TEXT" => {
                part.graphics.push(Graphic {
                    shape: Shape::Text {
                        at: point(row, 2, 3)?,
                        rotation: num(row, 4)?,
                        text: text(row, 5).into(),
                        size: 10.0,
                    },
                    layer: String::new(),
                    width: 1.0,
                });
            }
            _ => bail!("尚不支持符号图元 {kind}，已停止导出以避免不完整文件"),
        }
    }
    for row in &footprint {
        let kind = text(row, 0);
        match kind {
            "DOCTYPE" | "HEAD" | "LAYER" | "LAYER_PHYS" | "CANVAS" | "PREFERENCE"
            | "ACTIVE_LAYER" | "ATTR" | "" => {}
            "PAD" => {
                let shape = row
                    .get(10)
                    .and_then(Value::as_array)
                    .context("焊盘没有形状")?;
                let shape_name = text(shape, 0);
                ensure!(
                    matches!(
                        shape_name,
                        "RECT" | "ROUND" | "OVAL" | "ELLIPSE" | "OCTAGON"
                    ),
                    "尚不支持焊盘形状 {shape_name}"
                );
                ensure!(optional_num(shape, 3, 0.0) == 0.0, "暂不支持自定义圆角焊盘");
                let width = num(shape, 1)?;
                let height = num(shape, 2)?;
                ensure!(width > 0.0 && height > 0.0, "焊盘尺寸无效");
                let (hole, slot) = if let Some(h) = row.get(9).and_then(Value::as_array) {
                    let a = optional_num(h, 1, 0.0);
                    let b = optional_num(h, 2, a);
                    ensure!(matches!(text(h, 0), "ROUND" | "SLOT"), "不支持的钻孔形状");
                    if text(h, 0) == "SLOT" {
                        (a.min(b), a.max(b))
                    } else {
                        ensure!((a - b).abs() < 1e-6, "圆孔的两个直径不一致");
                        (a, a)
                    }
                } else {
                    let h = optional_num(row, 9, 0.0);
                    (h, h)
                };
                ensure!(
                    row.get(11)
                        .and_then(Value::as_array)
                        .is_none_or(Vec::is_empty),
                    "暂不支持逐层自定义焊盘堆叠"
                );
                ensure!(
                    hole == 0.0
                        || (optional_num(row, 12, 0.0) == 0.0 && optional_num(row, 13, 0.0) == 0.0),
                    "暂不支持偏心钻孔"
                );
                let pad_layer = layer(num(row, 4)? as i64)?;
                ensure!(
                    matches!(pad_layer, "TOP" | "BOTTOM" | "MULTILAYER"),
                    "焊盘层无效"
                );
                ensure!(
                    pad_layer != "MULTILAYER"
                        || optional_num(row, 19, 0.0) == optional_num(row, 20, 0.0),
                    "暂不支持多层焊盘的非对称助焊扩展"
                );
                component.pads.push(Pad {
                    number: text(row, 5).into(),
                    at: point(row, 6, 7)?,
                    width,
                    height,
                    rotation: num(row, 8)?,
                    shape: match shape_name {
                        "RECT" => "RECTANGLE",
                        "OCTAGON" => "OCTAGONAL",
                        _ => "ROUND",
                    }
                    .into(),
                    layer: pad_layer.into(),
                    hole,
                    slot,
                    hole_rotation: optional_num(row, 14, 0.0),
                    plated: optional_num(row, 15, 1.0) != 0.0,
                    mask_top: optional_num(row, 17, 0.0),
                    mask_bottom: optional_num(row, 18, 0.0),
                    paste_top: optional_num(row, 19, 0.0),
                    paste_bottom: optional_num(row, 20, 0.0),
                });
            }
            "POLY" | "FILL" => {
                let layer = layer(num(row, 4)? as i64)?.to_owned();
                let paths = if kind == "FILL" {
                    let paths = row
                        .get(7)
                        .and_then(Value::as_array)
                        .context("填充没有路径")?;
                    ensure!(paths.len() == 1, "暂不支持带内孔或多个轮廓的填充");
                    vec![&paths[0]]
                } else {
                    vec![row.get(6).context("POLY 没有路径")?]
                };
                for outline in paths {
                    component.graphics.push(Graphic {
                        shape: Shape::Path {
                            segments: path(outline)?,
                            filled: kind == "FILL",
                        },
                        layer: layer.clone(),
                        width: optional_num(row, 5, 1.0),
                    });
                }
            }
            _ => bail!("尚不支持封装图元 {kind}，已停止导出以避免不完整文件"),
        }
    }
    component.check()?;
    Ok(component)
}
