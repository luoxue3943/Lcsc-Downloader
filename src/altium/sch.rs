use super::{EASYEDA_URL, Record, SOURCE, SOURCE_URL, arc_angles, unique_id};
use crate::model::*;
use anyhow::{Result, ensure};

/// 创建关联父对象及子单元的原理图记录。 / Create a schematic record linked to its owner and part.
fn owned(kind: u32, owner: usize, part: usize) -> Record {
    Record::new(kind)
        .set("OWNERINDEX", owner)
        .set("OWNERPARTID", part)
        .set("OWNERPARTDISPLAYMODE", 0)
        .set("ISNOTACCESIBLE", "T")
}

/// 计算子单元的保守几何边界，供图纸尺寸和摆放使用。 / Compute conservative part bounds for sheet sizing and placement.
fn bounds(part: &SymbolPart) -> (f64, f64, f64, f64) {
    let mut points: Vec<Point> = part.pins.iter().map(|p| p.hot).collect();
    for graphic in &part.graphics {
        match &graphic.shape {
            Shape::Path { segments, .. } => {
                for s in segments {
                    points.extend([s.start(), s.end()]);
                    if let Some((c, r, _, _)) = arc_angles(s) {
                        points.extend([c.offset(-r, -r), c.offset(r, r)]);
                    }
                }
            }
            Shape::Ellipse { center, rx, ry, .. } => {
                points.extend([center.offset(-rx, -ry), center.offset(*rx, *ry)])
            }
            Shape::Text { at, .. } => points.push(*at),
        }
    }
    points
        .iter()
        .fold((0.0f64, 0.0f64, 0.0f64, 0.0f64), |(x0, y0, x1, y1), p| {
            (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y))
        })
}

/// 布局全部符号子单元并写出带来源及完整性统计的 SchDoc。 / Lay out all symbol parts and write a SchDoc with provenance and completeness counts.
pub(super) fn render(component: &Component) -> Result<Vec<u8>> {
    let bounds: Vec<_> = component.parts.iter().map(bounds).collect();
    let sheet_x = 1150.0f64.max(bounds.iter().map(|b| b.2 - b.0 + 100.0).sum::<f64>() + 100.0);
    let sheet_y = 760.0f64.max(bounds.iter().map(|b| b.3 - b.1 + 200.0).fold(0.0, f64::max));
    let mut records = vec![
        Record::new(31)
            .set("FONTIDCOUNT", 2)
            .set("SIZE1", 10)
            .set("FONTNAME1", "Arial")
            .set("SIZE2", 8)
            .set("FONTNAME2", "Arial")
            .set("USEMBCS", "T")
            .set("ISBOC", "T")
            .set("SHEETSTYLE", 0)
            .set("USECUSTOMSHEET", "T")
            .coord("CUSTOMX", sheet_x)
            .coord("CUSTOMY", sheet_y)
            .set("SYSTEMFONT", 1)
            .set("BORDERON", "T")
            .set("SNAPGRIDON", "T")
            .set("SNAPGRIDSIZE", 1)
            .set("VISIBLEGRIDON", "T")
            .set("VISIBLEGRIDSIZE", 10)
            .set("DISPLAY_UNIT", 0)
            .line()?,
    ];
    let mut cursor_x = 100.0;
    for (part_index, (part, bounds)) in component.parts.iter().zip(bounds).enumerate() {
        let part_id = part_index + 1;
        let owner = records.len();
        let offset = Point {
            x: cursor_x - bounds.0,
            y: 150.0 - bounds.1,
        };
        cursor_x += bounds.2 - bounds.0 + 100.0;
        let uid = unique_id(&format!("{}:part:{part_index}", component.id));
        records.push(
            Record::new(1)
                .set("LIBREFERENCE", &component.model)
                .set("COMPONENTDESCRIPTION", &component.description)
                .set("PARTCOUNT", component.parts.len() + 1)
                .set("DISPLAYMODECOUNT", 1)
                .set("OWNERPARTID", -1)
                .set("INDEXINSHEET", owner)
                .at(offset)
                .set("CURRENTPARTID", part_id)
                .set("ORIENTATION", 0)
                .set("SOURCELIBRARYNAME", "*")
                .set("TARGETFILENAME", "*")
                .set("UNIQUEID", uid)
                .set("DESIGNITEMID", &component.model)
                .set("ALLPINCOUNT", component.pin_count())
                .line()?,
        );
        for pin in &part.pins {
            // 按目标格式转换引脚定位端与方向，保持连接端点不变。 / Convert the pin anchor and direction for the target format while preserving its connection endpoint.
            let angle = pin.inward_angle.to_radians();
            let body = pin
                .hot
                .offset(pin.length * angle.cos(), pin.length * angle.sin())
                .offset(offset.x, offset.y);
            let orientation = ((pin.inward_angle + 180.0).rem_euclid(360.0) / 90.0).round() as u32;
            let flags = 32
                + orientation
                + if pin.show_name { 8 } else { 0 }
                + if pin.show_number { 16 } else { 0 };
            records.push(
                owned(2, owner, part_id)
                    .set("FORMALTYPE", 1)
                    .set("ELECTRICAL", pin.electrical)
                    .set("PINCONGLOMERATE", flags)
                    .coord("PINLENGTH", pin.length)
                    .at(body)
                    .set("NAME", &pin.name)
                    .set("DESIGNATOR", &pin.number)
                    .set("ISHIDDEN", if pin.hidden { "T" } else { "F" })
                    .set("SYMBOL_INNEREDGE", if pin.clock { 3 } else { 0 })
                    .set("SYMBOL_OUTEREDGE", u32::from(pin.inverted))
                    .set("COLOR", 128)
                    .set("NAME_CUSTOMFONTID", 2)
                    .set("DESIGNATOR_CUSTOMFONTID", 2)
                    .line()?,
            );
        }
        for graphic in &part.graphics {
            match &graphic.shape {
                Shape::Path { segments, filled } => {
                    ensure!(!filled, "尚不支持填充的符号路径");
                    for segment in segments {
                        let record = match segment {
                            Segment::Line { start, end } => owned(13, owner, part_id)
                                .at(start.offset(offset.x, offset.y))
                                .coord("CORNER.X", end.x + offset.x)
                                .coord("CORNER.Y", end.y + offset.y),
                            Segment::Arc { .. } => {
                                let (center, r, a, b) = arc_angles(segment).unwrap();
                                owned(12, owner, part_id)
                                    .at(center.offset(offset.x, offset.y))
                                    .coord("RADIUS", r)
                                    .set("STARTANGLE", super::decimal(a))
                                    .set("ENDANGLE", super::decimal(b))
                            }
                        };
                        records.push(record.set("COLOR", 128).set("LINEWIDTH", 1).line()?);
                    }
                }
                Shape::Ellipse {
                    center,
                    rx,
                    ry,
                    filled,
                } => records.push(
                    owned(8, owner, part_id)
                        .at(center.offset(offset.x, offset.y))
                        .coord("RADIUS", *rx)
                        .coord("SECONDARYRADIUS", *ry)
                        .set("LINEWIDTH", 1)
                        .set("COLOR", 128)
                        .set("ISSOLID", if *filled { "T" } else { "F" })
                        .line()?,
                ),
                Shape::Text {
                    at, text, rotation, ..
                } => {
                    ensure!(
                        (rotation / 90.0 - (rotation / 90.0).round()).abs() < 1e-6,
                        "仅支持正交符号文字"
                    );
                    records.push(
                        owned(4, owner, part_id)
                            .at(at.offset(offset.x, offset.y))
                            .set("TEXT", text)
                            .set("FONTID", 1)
                            .set("JUSTIFICATION", 4)
                            .set(
                                "ORIENTATION",
                                (rotation.rem_euclid(360.0) / 90.0).round() as u32,
                            )
                            .set("COLOR", 128)
                            .line()?,
                    );
                }
            }
        }
        let label = offset.offset(bounds.0, bounds.3 + 15.0);
        records.push(
            owned(34, owner, part_id)
                .at(label)
                .set("COLOR", 8388608)
                .set("FONTID", 1)
                .set("TEXT", &component.designator)
                .set("NAME", "Designator")
                .set("READONLYSTATE", 1)
                .line()?,
        );
        for (name, value, hidden) in [
            ("Comment", component.model.as_str(), false),
            ("LCSC_ID", component.id.as_str(), true),
            ("Footprint", component.model.as_str(), true),
            ("Source", SOURCE, true),
            ("SourceURL", SOURCE_URL, true),
            ("EasyEDAURL", EASYEDA_URL, true),
            ("SourceLicense", component.source_license.as_str(), true),
        ] {
            records.push(
                owned(41, owner, part_id)
                    .at(label.offset(0.0, 12.0))
                    .set("FONTID", 1)
                    .set("COLOR", 8388608)
                    .set("NAME", name)
                    .set("TEXT", value)
                    .set("ISHIDDEN", if hidden { "T" } else { "F" })
                    .line()?,
            );
        }
    }
    // 保存源数量统计，使已有输出能够重新检查完整性。 / Retain source counts so existing output can be checked again for completeness.
    for (name, value) in [
        ("LCSC_SOURCE_PIN_COUNT", component.pin_count().to_string()),
        ("LCSC_SOURCE_PAD_COUNT", component.pads.len().to_string()),
    ] {
        records.push(
            Record::new(41)
                .set("OWNERINDEX", -1)
                .set("OWNERPARTID", -1)
                .set("NAME", name)
                .set("TEXT", value)
                .set("ISHIDDEN", "T")
                .line()?,
        );
    }
    let mut bytes = Record::default()
        .set(
            "HEADER",
            "Protel for Windows - Schematic Capture Ascii File Version 5.0",
        )
        .set("WEIGHT", records.len())
        .set("MINORVERSION", 2)
        .set("UNIQUEID", unique_id(&component.id))
        .line()?;
    for record in records {
        bytes.extend(record);
    }
    bytes.extend(Record::default().set("HEADER", "Icon storage").line()?);
    Ok(bytes)
}
