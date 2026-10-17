//! 统一二维器件模型；原理图坐标以 10 mil 为单位，PCB 坐标以 mil 为单位。 / Unified 2D component model; schematic coordinates use 10 mil units and PCB coordinates use mil.
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    /// 仅在两个坐标均为有限数值时构造二维点。 / Construct a 2D point only when both coordinates are finite.
    pub fn new(x: f64, y: f64) -> Result<Self> {
        ensure!(x.is_finite() && y.is_finite(), "坐标不是有限数值");
        Ok(Self { x, y })
    }

    /// 按指定增量平移坐标并返回新点。 / Translate the coordinates by the given offsets and return a new point.
    pub fn offset(self, x: f64, y: f64) -> Self {
        Self {
            x: self.x + x,
            y: self.y + y,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Segment {
    Line {
        start: Point,
        end: Point,
    },
    Arc {
        start: Point,
        end: Point,
        center: Point,
        sweep: f64,
    },
}
impl Segment {
    /// 取得直线或圆弧的起点。 / Return the starting point of a line or arc.
    pub fn start(&self) -> Point {
        match *self {
            Self::Line { start, .. } | Self::Arc { start, .. } => start,
        }
    }

    /// 取得直线或圆弧的终点。 / Return the endpoint of a line or arc.
    pub fn end(&self) -> Point {
        match *self {
            Self::Line { end, .. } | Self::Arc { end, .. } => end,
        }
    }

    /// 由不同端点及有效扫角计算圆心并构造圆弧。 / Compute the center from distinct endpoints and a valid sweep angle to construct an arc.
    pub fn arc(start: Point, end: Point, sweep: f64) -> Result<Self> {
        ensure!(
            sweep.is_finite() && sweep.abs() > 0.000001 && sweep.abs() < 360.0,
            "不支持的圆弧角度：{sweep}"
        );
        let (dx, dy) = (end.x - start.x, end.y - start.y);
        ensure!(dx.hypot(dy) > 1e-8, "圆弧端点重合");
        let factor = 0.5 / (sweep.to_radians() / 2.0).tan();
        let center = Point::new(
            (start.x + end.x) / 2.0 - dy * factor,
            (start.y + end.y) / 2.0 + dx * factor,
        )?;
        Ok(Self::Arc {
            start,
            end,
            center,
            sweep,
        })
    }
}

#[derive(Clone, Debug)]
pub enum Shape {
    Path {
        segments: Vec<Segment>,
        filled: bool,
    },
    Ellipse {
        center: Point,
        rx: f64,
        ry: f64,
        filled: bool,
    },
    Text {
        at: Point,
        text: String,
        rotation: f64,
        size: f64,
    },
}
#[derive(Clone, Debug)]
pub struct Graphic {
    pub shape: Shape,
    pub layer: String,
    pub width: f64,
}
#[derive(Clone, Debug)]
pub struct Pin {
    pub number: String,
    pub name: String,
    pub hot: Point,
    pub length: f64,
    pub inward_angle: f64,
    pub electrical: u8,
    pub show_name: bool,
    pub show_number: bool,
    pub inverted: bool,
    pub clock: bool,
    pub hidden: bool,
}
#[derive(Clone, Debug)]
pub struct SymbolPart {
    pub name: String,
    pub pins: Vec<Pin>,
    pub graphics: Vec<Graphic>,
}
#[derive(Clone, Debug)]
pub struct Pad {
    pub number: String,
    pub at: Point,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
    pub shape: String,
    pub layer: String,
    pub hole: f64,
    pub slot: f64,
    pub hole_rotation: f64,
    pub plated: bool,
    pub mask_top: f64,
    pub mask_bottom: f64,
    pub paste_top: f64,
    pub paste_bottom: f64,
}
#[derive(Clone, Debug)]
pub struct Component {
    pub id: String,
    pub model: String,
    pub description: String,
    pub designator: String,
    pub source_license: String,
    pub parts: Vec<SymbolPart>,
    pub pads: Vec<Pad>,
    pub graphics: Vec<Graphic>,
}
impl Component {
    /// 累计所有符号子单元的引脚数量。 / Count pins across all symbol parts.
    pub fn pin_count(&self) -> usize {
        self.parts.iter().map(|part| part.pins.len()).sum()
    }

    /// 要求器件包含引脚与焊盘，并核对两者的非空编号集合。 / Require pins and pads and verify their nonempty identifier sets match.
    pub fn check(&self) -> Result<()> {
        ensure!(
            !self.parts.is_empty() && self.pin_count() > 0,
            "原理图没有可用引脚"
        );
        ensure!(!self.pads.is_empty(), "封装没有可用焊盘");
        let pins: std::collections::BTreeSet<_> = self
            .parts
            .iter()
            .flat_map(|p| &p.pins)
            .map(|p| p.number.as_str())
            .collect();
        let pads: std::collections::BTreeSet<_> = self
            .pads
            .iter()
            .filter(|p| !p.number.is_empty())
            .map(|p| p.number.as_str())
            .collect();
        ensure!(
            pins == pads,
            "引脚与焊盘编号不一致：仅符号 {:?}；仅封装 {:?}",
            pins.difference(&pads).collect::<Vec<_>>(),
            pads.difference(&pins).collect::<Vec<_>>()
        );
        Ok(())
    }
}
