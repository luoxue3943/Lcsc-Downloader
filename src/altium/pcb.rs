use super::{EASYEDA_URL, Record, SOURCE, SOURCE_URL, arc_angles, decimal, mil, unique_id};
use crate::model::*;
use anyhow::{Result, ensure};

// 统一偏移封装坐标，保持焊盘与图形的相对布局。 / Apply a shared footprint offset while preserving the relative layout of pads and graphics.
const ORIGIN: f64 = 2000.0;
/// 创建关联封装与图层的未锁定普通图形记录。 / Create an unlocked ordinary graphic record linked to its component and layer.
fn graphic_record(kind: &str, layer: &str) -> Record {
    Record::new(kind)
        .set("COMPONENT", 0)
        .set("LAYER", layer)
        .set("LOCKED", "FALSE")
        .set("KEEPOUT", "FALSE")
}

/// 要求单轮廓首尾闭合，并编码包含直线或圆弧的填充区域。 / Require a closed single contour and encode its line or arc segments as a filled region.
fn region(segments: &[Segment], layer: &str) -> Result<Vec<u8>> {
    ensure!(!segments.is_empty(), "填充轮廓为空");
    let first = segments[0].start();
    let last = segments.last().unwrap().end();
    ensure!(
        (first.x - last.x).hypot(first.y - last.y) < 1e-5,
        "填充轮廓没有闭合"
    );
    let mut record = graphic_record("Region", layer)
        .set("KIND", 0)
        .set("HOLECOUNT", 0)
        .set("ISBOARDCUTOUT", "FALSE")
        .set("ISSHAPEBASED", "TRUE")
        .set("POLYGONOUTLINE", "FALSE")
        .set("MAINCONTOURVERTEXCOUNT", segments.len() + 1);
    for (index, segment) in segments.iter().enumerate() {
        let point = segment.start();
        record = record
            .set(format!("VX{index}"), mil(point.x + ORIGIN))
            .set(format!("VY{index}"), mil(point.y + ORIGIN));
        if let Some((center, r, a, b)) = arc_angles(segment) {
            record = record
                .set(format!("KIND{index}"), 1)
                .set(format!("CX{index}"), mil(center.x + ORIGIN))
                .set(format!("CY{index}"), mil(center.y + ORIGIN))
                .set(format!("R{index}"), mil(r))
                .set(format!("SA{index}"), decimal(a))
                .set(format!("EA{index}"), decimal(b));
        } else {
            record = record.set(format!("KIND{index}"), 0);
        }
    }
    record
        .set(format!("VX{}", segments.len()), mil(first.x + ORIGIN))
        .set(format!("VY{}", segments.len()), mil(first.y + ORIGIN))
        .set(format!("KIND{}", segments.len()), 0)
        .line()
}

/// 写出板框、封装、焊盘及图形，并附带 PcbDoc 完整性标记。 / Write the board, component, pads, and graphics with a PcbDoc completeness marker.
pub(super) fn render(component: &Component) -> Result<Vec<u8>> {
    let mut records = Vec::new();
    let extent = component
        .pads
        .iter()
        .map(|p| p.at.x.abs().max(p.at.y.abs()) + p.width.max(p.height))
        .fold(100.0, f64::max)
        + 300.0;
    let mut board = Record::new("Board")
        .set("KIND", "Protel_Advanced_PCB")
        .set("VERSION", "5.00")
        .set("LAYER", "UNKNOWN")
        .set("DISPLAYUNIT", 0)
        .set("ISSHAPEBASED", "TRUE")
        .set("POLYGONOUTLINE", "FALSE")
        .set("MAINCONTOURVERTEXCOUNT", 5)
        .set("SHEETWIDTH", mil(ORIGIN * 2.0 + extent))
        .set("SHEETHEIGHT", mil(ORIGIN * 2.0 + extent));
    for (index, (x, y)) in [
        (-extent, -extent),
        (extent, -extent),
        (extent, extent),
        (-extent, extent),
        (-extent, -extent),
    ]
    .into_iter()
    .enumerate()
    {
        board = board
            .set(format!("KIND{index}"), 0)
            .set(format!("VX{index}"), mil(x + ORIGIN))
            .set(format!("VY{index}"), mil(y + ORIGIN));
    }
    for (id, name) in [
        (1, "Top Layer"),
        (32, "Bottom Layer"),
        (33, "Top Overlay"),
        (34, "Bottom Overlay"),
        (35, "Top Paste"),
        (36, "Bottom Paste"),
        (37, "Top Solder"),
        (38, "Bottom Solder"),
        (74, "Multi-Layer"),
    ] {
        board = board.set(format!("LAYER{id}NAME"), name);
    }
    board = board
        .set("LAYER1NEXT", 32)
        .set("LAYER32PREV", 1)
        .set("LAYER1DIELHEIGHT", "62mil")
        .set("LAYER1DIELMATERIAL", "FR-4");
    for index in 1..=9 {
        board = board
            .set(
                format!("LAYER{}NAME", index + 56),
                format!("Mechanical {index}"),
            )
            .set(format!("LAYER{}MECHENABLED", index + 56), "TRUE");
    }
    records.push(board.line()?);
    records.push(
        Record::new("Component")
            .set("ID", 0)
            .set("LAYER", "TOP")
            .set("X", mil(ORIGIN))
            .set("Y", mil(ORIGIN))
            .set("ROTATION", 0)
            .set("LOCKED", "FALSE")
            .set("PATTERN", &component.model)
            .set("SOURCEDESIGNATOR", &component.designator)
            .set(
                "SOURCEUNIQUEID",
                unique_id(&format!("{}:part:0", component.id)),
            )
            .set("NAMEON", "TRUE")
            .set("COMMENTON", "TRUE")
            .set("LCSC_ID", &component.id)
            .set("SOURCE", SOURCE)
            .set("SOURCEURL", SOURCE_URL)
            .set("EASYEDAURL", EASYEDA_URL)
            .set("SOURCE_LICENSE", &component.source_license)
            .line()?,
    );
    for pad in &component.pads {
        let record = graphic_record("Pad", &pad.layer)
            .set("X", mil(pad.at.x + ORIGIN))
            .set("Y", mil(pad.at.y + ORIGIN))
            .set("NAME", &pad.number)
            .set("ROTATION", decimal(pad.rotation))
            .set("XSIZE", mil(pad.width))
            .set("YSIZE", mil(pad.height))
            .set("SHAPE", &pad.shape)
            .set("TOPSHAPE", &pad.shape)
            .set("MIDSHAPE", &pad.shape)
            .set("BOTSHAPE", &pad.shape)
            .set("TOPXSIZE", mil(pad.width))
            .set("TOPYSIZE", mil(pad.height))
            .set("MIDXSIZE", mil(pad.width))
            .set("MIDYSIZE", mil(pad.height))
            .set("BOTXSIZE", mil(pad.width))
            .set("BOTYSIZE", mil(pad.height))
            .set("HOLESIZE", mil(pad.hole))
            .set("HOLETYPE", if pad.slot > pad.hole { 2 } else { 0 })
            .set("HOLEWIDTH", mil(pad.slot))
            .set("HOLEROTATION", decimal(pad.hole_rotation))
            .set("PLATED", if pad.plated { "TRUE" } else { "FALSE" })
            .set("PADMODE", 0)
            .set("SOLDERMASKEXPANSIONMODE", "Manual")
            .set("CSE", mil(pad.mask_top))
            .set("CSB", mil(pad.mask_bottom))
            .set("CUSE", "TRUE")
            .set("PASTEMASKEXPANSIONMODE", "Manual")
            .set(
                "CPE",
                mil(if pad.layer == "BOTTOM" {
                    pad.paste_bottom
                } else {
                    pad.paste_top
                }),
            )
            .set("TENTINGTOP", "FALSE")
            .set("TENTINGBOTTOM", "FALSE");
        records.push(record.line()?);
    }
    for graphic in &component.graphics {
        match &graphic.shape {
            Shape::Path { segments, filled } => {
                if *filled {
                    records.push(region(segments, &graphic.layer)?);
                    continue;
                }
                for segment in segments {
                    let record = match segment {
                        Segment::Line { start, end } => graphic_record("Track", &graphic.layer)
                            .set("X1", mil(start.x + ORIGIN))
                            .set("Y1", mil(start.y + ORIGIN))
                            .set("X2", mil(end.x + ORIGIN))
                            .set("Y2", mil(end.y + ORIGIN)),
                        Segment::Arc { .. } => {
                            let (center, r, a, b) = arc_angles(segment).unwrap();
                            graphic_record("Arc", &graphic.layer)
                                .set("LOCATION.X", mil(center.x + ORIGIN))
                                .set("LOCATION.Y", mil(center.y + ORIGIN))
                                .set("RADIUS", mil(r))
                                .set("STARTANGLE", decimal(a))
                                .set("ENDANGLE", decimal(b))
                        }
                    };
                    records.push(record.set("WIDTH", mil(graphic.width)).line()?);
                }
            }
            Shape::Ellipse { .. } => anyhow::bail!("封装椭圆必须先转换成圆弧路径"),
            Shape::Text {
                at,
                text,
                rotation,
                size,
            } => records.push(
                graphic_record("Text", &graphic.layer)
                    .set("X", mil(at.x + ORIGIN))
                    .set("Y", mil(at.y + ORIGIN))
                    .set("ROTATION", decimal(*rotation))
                    .set("HEIGHT", mil(*size))
                    .set("WIDTH", "6mil")
                    .set("TEXT", text)
                    .set("FONTNAME", "Arial")
                    .set(
                        "WIDESTRING",
                        text.encode_utf16()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    )
                    .line()?,
            ),
        }
    }
    for (index, (text, designator)) in [(&component.designator, true), (&component.model, false)]
        .into_iter()
        .enumerate()
    {
        records.push(
            graphic_record("Text", "TOPOVERLAY")
                .set("X", mil(ORIGIN))
                .set("Y", mil(ORIGIN + extent - 100.0 - index as f64 * 80.0))
                .set("HEIGHT", "60mil")
                .set("WIDTH", "6mil")
                .set("TEXT", text)
                .set("FONTNAME", "Arial")
                .set(
                    "WIDESTRING",
                    text.encode_utf16()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                )
                .set(if designator { "DESIGNATOR" } else { "COMMENT" }, "TRUE")
                .line()?,
        );
    }
    let record_count = records.len() + 1;
    records.push(
        Record::new("Board")
            .set("LCSC_ID", &component.id)
            .set("LCSC_SOURCE_PIN_COUNT", component.pin_count())
            .set("LCSC_SOURCE_PAD_COUNT", component.pads.len())
            .set("LCSC_EXPORT_RECORD_COUNT", record_count)
            .set("LCSC_EXPORT_COMPLETE", "TRUE")
            .line()?,
    );
    Ok(records.into_iter().flatten().collect())
}
