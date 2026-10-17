use anyhow::Result;
use lcsc_downloader::{
    altium,
    config::{self, Settings},
    download::{self, DownloadRequest},
    input::{decode_text, parse_ids, safe_name},
    source,
    validation::{records, validate_complete, validate_documents},
};
use serde_json::{Value, json};
use std::{fs, sync::mpsc};
use tokio_util::sync::CancellationToken;

/// 读取指定器件与资源类型的本地 JSON 回归样例。 / Read the local JSON regression fixture for a component and resource kind.
fn fixture(id: &str, kind: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{id}-{kind}.json"));
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// 将产品、符号及封装样例转换为待测试器件模型。 / Convert product, symbol, and footprint fixtures into the component model under test.
fn component(id: &str) -> lcsc_downloader::model::Component {
    source::convert(
        &fixture(id, "product"),
        &fixture(id, "symbol"),
        &fixture(id, "footprint"),
    )
    .unwrap()
}

/// 读取测试记录的数值字段，并移除可选的 mil 后缀。 / Read a numeric field from a test record and remove an optional mil suffix.
fn number(row: &std::collections::BTreeMap<String, String>, key: &str) -> f64 {
    row[key].trim_end_matches("mil").parse().unwrap()
}

/// 合并整数及十万分之一小数字段，还原导出坐标。 / Reconstruct an exported coordinate from its integer and one hundred-thousandth fractional fields.
fn coord(row: &std::collections::BTreeMap<String, String>, key: &str) -> f64 {
    number(row, key)
        + row
            .get(&format!("{key}_FRAC"))
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
            / 100_000.0
}

/// 验证编号提取去重、文件名清理和过长型号的区分能力。 / Verify ID extraction and deduplication, filename sanitization, and distinction of long model names.
#[test]
fn parse_ids_and_names() -> Result<()> {
    assert_eq!(
        parse_ids("c8734 C2040，C8734\nC25804;ABC8734 STM32C8734 https://x/C999.html"),
        ["C8734", "C2040", "C25804", "C999"]
    );
    assert!(parse_ids("C1abc C 123").is_empty());
    assert_eq!(safe_name("A/B:C*D? ")?, "A_B_C_D_");
    assert_eq!(safe_name("CON")?, "_CON");
    assert_eq!(safe_name("..MODEL..")?, "MODEL");
    assert!(safe_name("...").is_err());
    assert_ne!(
        safe_name(&format!("{}A", "X".repeat(150)))?,
        safe_name(&format!("{}B", "X".repeat(150)))?
    );
    Ok(())
}

/// 验证 UTF-8、UTF-16 与 GBK 清单文本的解码结果。 / Verify decoded results for UTF-8, UTF-16, and GBK list text.
#[test]
fn text_import_encodings() {
    assert_eq!(decode_text(b"\xef\xbb\xbfC8734"), "C8734");
    assert_eq!(decode_text(b"\xff\xfeC\x008\x007\x003\x004\x00"), "C8734");
    let (bytes, _, _) = encoding_rs::GBK.encode("元器件 C8734");
    assert_eq!(decode_text(&bytes), "元器件 C8734");
}

/// 验证配置读写、旧字段迁移、并发限制及损坏回退。 / Verify settings persistence, legacy aliases, concurrency limits, and recovery from corrupt data.
#[test]
fn settings_remember_and_migrate() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config/settings.json");
    let expected = Settings {
        download_directory: root.path().join("中文目录"),
        parallel: 3,
    };
    config::save(&path, &expected)?;
    assert_eq!(config::load(&path), expected);
    config::save(
        &path,
        &Settings {
            parallel: 1,
            ..expected.clone()
        },
    )?;
    assert_eq!(config::load(&path).parallel, 1);
    fs::write(
        &path,
        serde_json::to_vec(&json!({"DownloadDirectory":expected.download_directory,"Parallel":3}))?,
    )?;
    assert_eq!(config::load(&path), expected);
    fs::write(&path, b"broken")?;
    assert_eq!(config::load(&path), Settings::default());
    config::save(
        &path,
        &Settings {
            parallel: 999,
            ..expected
        },
    )?;
    assert_eq!(config::load(&path).parallel, 4);
    Ok(())
}

/// 验证真实样例生成两份可校验的 ASCII Doc 文件及预期数量的引脚与焊盘。 / Verify real fixtures produce two valid ASCII Doc files with the expected pin and pad counts.
#[test]
fn real_source_exports_exactly_two_docs() -> Result<()> {
    for (id, pins, pads) in [("C8734", 48, 48), ("C2040", 57, 57), ("C25804", 2, 2)] {
        let c = component(id);
        assert_eq!(c.pin_count(), pins);
        assert_eq!(c.pads.len(), pads);
        let docs = altium::render(&c)?;
        validate_documents(&docs.schematic, &docs.pcb, &c.model, id)?;
        let root = tempfile::tempdir()?;
        fs::write(
            root.path().join(format!("{}.SchDoc", c.model)),
            &docs.schematic,
        )?;
        fs::write(root.path().join(format!("{}.PcbDoc", c.model)), &docs.pcb)?;
        validate_complete(root.path(), &c.model, id)?;
        assert_eq!(fs::read_dir(root.path())?.count(), 2);
        assert!(!docs.schematic.starts_with(&[0xd0, 0xcf]));
    }
    Ok(())
}

/// 逐焊盘比对导出编号、位置、尺寸、旋转与阻焊扩展。 / Compare each exported pad against its source ID, position, size, rotation, and solder mask expansion.
#[test]
fn pad_dimensions_positions_rotation_and_masks_match_source() -> Result<()> {
    for id in ["C8734", "C2040", "C25804"] {
        let c = component(id);
        let docs = altium::render(&c)?;
        let rows = records(&docs.pcb)?;
        for (source, exported) in c.pads.iter().zip(
            rows.iter()
                .filter(|r| r.get("RECORD").is_some_and(|s| s == "Pad")),
        ) {
            assert_eq!(source.number, exported["NAME"]);
            for (actual, expected) in [
                (number(exported, "X") - 2000.0, source.at.x),
                (number(exported, "Y") - 2000.0, source.at.y),
                (number(exported, "XSIZE"), source.width),
                (number(exported, "YSIZE"), source.height),
                (number(exported, "ROTATION"), source.rotation),
                (number(exported, "CSE"), source.mask_top),
                (number(exported, "CSB"), source.mask_bottom),
            ] {
                assert!(
                    (actual - expected).abs() < 1e-5,
                    "{id}: {actual} != {expected}"
                );
            }
        }
    }
    Ok(())
}

/// 反算导出引脚的连接端点，并核对编号与电气类型。 / Reconstruct exported pin connection endpoints and verify their IDs and electrical types.
#[test]
fn pin_hotspots_and_electrical_types_match_source() -> Result<()> {
    for id in ["C8734", "C2040", "C25804"] {
        let c = component(id);
        let docs = altium::render(&c)?;
        let rows = records(&docs.schematic)?;
        let rows = &rows[1..rows.len() - 1];
        for (source, exported) in c.parts.iter().flat_map(|p| &p.pins).zip(
            rows.iter()
                .filter(|r| r.get("RECORD").is_some_and(|s| s == "2")),
        ) {
            let owner = &rows[exported["OWNERINDEX"].parse::<usize>()?];
            let direction = (exported["PINCONGLOMERATE"].parse::<u32>()? & 3) as f64 * 90.0;
            let direction = direction.to_radians();
            let length = coord(exported, "PINLENGTH");
            let x = coord(exported, "LOCATION.X") - coord(owner, "LOCATION.X")
                + direction.cos() * length;
            let y = coord(exported, "LOCATION.Y") - coord(owner, "LOCATION.Y")
                + direction.sin() * length;
            assert!(
                (x - source.hot.x).abs() < 1e-5 && (y - source.hot.y).abs() < 1e-5,
                "hotspot changed for {id} {}",
                source.number
            );
            assert_eq!(source.number, exported["DESIGNATOR"]);
            assert_eq!(source.electrical.to_string(), exported["ELECTRICAL"]);
        }
    }
    Ok(())
}

/// 验证截断文档、错误器件编号和缺失焊盘均被拒绝。 / Verify rejection of truncated documents, incorrect component IDs, and missing pads.
#[test]
fn corrupt_or_incomplete_documents_are_rejected() -> Result<()> {
    let c = component("C25804");
    let docs = altium::render(&c)?;
    assert!(
        validate_documents(
            &docs.schematic[..docs.schematic.len() - 20],
            &docs.pcb,
            &c.model,
            &c.id
        )
        .is_err()
    );
    assert!(
        validate_documents(
            &docs.schematic,
            &docs.pcb[..docs.pcb.len() - 20],
            &c.model,
            &c.id
        )
        .is_err()
    );
    assert!(validate_documents(&docs.schematic, &docs.pcb, &c.model, "C999").is_err());
    let missing = String::from_utf8(docs.pcb.clone())?
        .lines()
        .filter(|l| !l.starts_with("|RECORD=Pad|") || !l.contains("|NAME=1|"))
        .collect::<Vec<_>>()
        .join("\r\n");
    assert!(validate_documents(&docs.schematic, missing.as_bytes(), &c.model, &c.id).is_err());
    Ok(())
}

/// 验证未知图元、编号不匹配及字段分隔符注入均显式失败。 / Verify explicit failure for unknown primitives, mismatched IDs, and field delimiter injection.
#[test]
fn unsupported_shapes_and_invalid_mapping_fail_explicitly() -> Result<()> {
    let mut symbol = fixture("C25804", "symbol");
    symbol["result"]["dataStr"] = Value::String(format!(
        "{}\n[\"UNKNOWN_GRAPHIC\",\"x\"]",
        symbol["result"]["dataStr"].as_str().unwrap()
    ));
    assert!(
        source::convert(
            &fixture("C25804", "product"),
            &symbol,
            &fixture("C25804", "footprint")
        )
        .unwrap_err()
        .to_string()
        .contains("UNKNOWN_GRAPHIC")
    );
    let mut c = component("C25804");
    c.pads[0].number = "999".into();
    assert!(altium::render(&c).is_err());
    let mut c = component("C25804");
    c.parts[0].pins[0].name = "injected|RECORD=2".into();
    assert!(altium::render(&c).is_err());
    Ok(())
}

/// 验证拆分后的多个符号子单元全部出现在原理图中。 / Verify that all split symbol parts appear in the schematic.
#[test]
fn multi_unit_symbols_are_all_placed() -> Result<()> {
    let mut c = component("C25804");
    let pin = c.parts[0].pins.pop().unwrap();
    c.parts.push(lcsc_downloader::model::SymbolPart {
        name: "SECOND".into(),
        pins: vec![pin],
        graphics: c.parts[0].graphics.clone(),
    });
    let docs = altium::render(&c)?;
    validate_documents(&docs.schematic, &docs.pcb, &c.model, &c.id)?;
    assert_eq!(
        records(&docs.schematic)?
            .iter()
            .filter(|r| r.get("RECORD").is_some_and(|v| v == "1"))
            .count(),
        2
    );
    Ok(())
}

/// 验证中文型号和引脚名，以及时钟与反相标记得以保留。 / Verify preservation of Chinese model and pin names, clock markers, and inversion flags.
#[test]
fn unicode_and_clock_inversion_flags_are_preserved() -> Result<()> {
    let mut c = component("C25804");
    c.model = "电阻测试".into();
    c.parts[0].pins[0].name = "时钟输入".into();
    c.parts[0].pins[0].clock = true;
    c.parts[0].pins[0].inverted = true;
    let docs = altium::render(&c)?;
    validate_documents(&docs.schematic, &docs.pcb, &c.model, &c.id)?;
    let sch = records(&docs.schematic)?;
    let pin = sch
        .iter()
        .find(|r| r.get("NAME").is_some_and(|v| v == "时钟输入"))
        .unwrap();
    assert_eq!(pin["SYMBOL_INNEREDGE"], "3");
    assert_eq!(pin["SYMBOL_OUTEREDGE"], "1");
    Ok(())
}

/// 以已知四分之一圆验证端点与扫角计算得到的圆心。 / Verify the center derived from endpoints and sweep using a known quarter circle.
#[test]
fn arcs_keep_true_circle_geometry() -> Result<()> {
    use lcsc_downloader::model::{Point, Segment};
    if let Segment::Arc { center, .. } =
        Segment::arc(Point::new(1.0, 0.0)?, Point::new(0.0, 1.0)?, 90.0)?
    {
        assert!(center.x.abs() < 1e-8 && center.y.abs() < 1e-8);
    } else {
        panic!("not an arc");
    }
    Ok(())
}

/// 验证预先取消的批次按唯一编号报告终态且不生成文件。 / Verify that a precancelled batch reports one terminal event per unique ID without creating files.
#[tokio::test]
async fn cancellation_and_batch_deduplication() -> Result<()> {
    let root = tempfile::tempdir()?;
    let (tx, rx) = mpsc::channel();
    let cancel = CancellationToken::new();
    cancel.cancel();
    download::run(
        DownloadRequest {
            ids: vec!["C8734".into(), "C2040".into(), "C8734".into()],
            directory: root.path().to_owned(),
            parallel: 1,
        },
        tx,
        cancel,
    )
    .await?;
    let events: Vec<_> = rx.iter().collect();
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|e| e.finished && !e.success && e.status == "已取消")
    );
    assert_eq!(fs::read_dir(root.path())?.count(), 0);
    Ok(())
}

/// 验证空列表和非法编号被拒绝，规范编号被接受。 / Verify that empty lists and invalid IDs are rejected while normalized IDs are accepted.
#[test]
fn invalid_request_ids() {
    assert!(download::validate_ids(&[]).is_err());
    assert!(download::validate_ids(&["wrong".into()]).is_err());
    assert!(download::validate_ids(&["C8734".into()]).is_ok());
}

/// 联网下载三个器件，并验证每个成功结果仅包含两份有效 Doc。 / Download three components from the live service and verify each success contains exactly two valid Doc files.
#[tokio::test]
#[ignore = "requires live LCSC network access"]
async fn live_download_two_docs_per_component() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = std::env::var_os("LCSC_LIVE_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned());
    let (tx, rx) = mpsc::channel();
    download::run(
        DownloadRequest {
            ids: vec!["C8734".into(), "C2040".into(), "C25804".into()],
            directory: root,
            parallel: 2,
        },
        tx,
        CancellationToken::new(),
    )
    .await?;
    let results: Vec<_> = rx.iter().filter(|e| e.finished).collect();
    assert_eq!(results.len(), 3);
    for result in results {
        assert!(result.success, "{}: {:?}", result.id, result.detail);
        let folder = result.folder.unwrap();
        validate_complete(
            &folder,
            &safe_name(result.model.as_ref().unwrap())?,
            &result.id,
        )?;
        assert_eq!(fs::read_dir(&folder)?.count(), 2);
        println!("{} -> {}", result.id, folder.display());
    }
    Ok(())
}
