/// ChartData 구조체 / ChartData structure
///
/// HWP 차트 데이터는 ChartObj들이 순차적으로 나열된 바이너리 형식입니다.
/// HWP chart data is a binary format where ChartObj elements are listed sequentially.
///
/// **스펙 참고 / Spec Reference**
/// - 한글문서파일형식_차트_revision1.2.pdf (cdn.hancom.com)
///
/// **ChartObj 기본 구조 / ChartObj Basic Structure**
/// ```text
/// | id (long) | StoredtypeId (long) | StoredName (char*) | StoredVersion (int) | ChartObjData |
/// ```
/// - StoredName과 StoredVersion은 Variable Data
/// - 동일한 StoredtypeId가 앞에 있으면 Variable Data는 생략됨
///
/// **VtChart Tree 구조 / VtChart Tree Structure**
/// ```text
/// VtChart
/// ├── BackDrop
/// ├── DataGrid
/// ├── Footnote
/// ├── Legend
/// ├── Plot
/// ├── PrintInformation
/// └── Title
/// ```
use crate::error::HwpError;
use crate::types::INT32;
use serde::{Deserialize, Serialize};

// ============================================================================
// Constants (스펙 페이지 24-39)
// ============================================================================

/// 차트 유형 상수 / Chart Type Constants (표 70)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum ChartType {
    /// 3D 막대 / 3D Bar
    #[default]
    Bar3D = 0,
    /// 2D 막대 / 2D Bar
    Bar2D = 1,
    /// 3D 선 / 3D Line
    Line3D = 2,
    /// 2D 선 / 2D Line
    Line2D = 3,
    /// 3D 영역 / 3D Area
    Area3D = 4,
    /// 2D 영역 / 2D Area
    Area2D = 5,
    /// 3D 계단 / 3D Step
    Step3D = 6,
    /// 2D 계단 / 2D Step
    Step2D = 7,
    /// 3D 조합 / 3D Combo
    Combo3D = 8,
    /// 2D 조합 / 2D Combo
    Combo2D = 9,
    /// 3D 가로 막대 / 3D Horizontal Bar
    HorizontalBar3D = 10,
    /// 2D 가로 막대 / 2D Horizontal Bar
    HorizontalBar2D = 11,
    /// 3D 클러스터 막대 / 3D Cluster Bar
    ClusterBar3D = 12,
    /// 3D 파이 / 3D Pie
    Pie3D = 13,
    /// 2D 파이 / 2D Pie
    Pie2D = 14,
    /// 2D 도넛 / 2D Doughnut
    Doughnut2D = 15,
    /// 2D XY / 2D XY (Scatter)
    XY2D = 16,
    /// 2D 원추 / 2D Polar
    Polar2D = 17,
    /// 2D 방사 / 2D Radar
    Radar2D = 18,
    /// 2D 풍선 / 2D Bubble
    Bubble2D = 19,
    /// 2D Hi-Lo / 2D Hi-Lo
    HiLo2D = 20,
    /// 2D 간트 / 2D Gantt
    Gantt2D = 21,
    /// 3D 간트 / 3D Gantt
    Gantt3D = 22,
    /// 3D 평면 / 3D Surface
    Surface3D = 23,
    /// 2D 등고선 / 2D Contour
    Contour2D = 24,
    /// 3D 산포 / 3D Scatter
    Scatter3D = 25,
    /// 3D XYZ / 3D XYZ
    XYZ3D = 26,
    /// 알 수 없는 타입 / Unknown type
    Unknown = 255,
}

impl From<u32> for ChartType {
    fn from(value: u32) -> Self {
        match value {
            0 => ChartType::Bar3D,
            1 => ChartType::Bar2D,
            2 => ChartType::Line3D,
            3 => ChartType::Line2D,
            4 => ChartType::Area3D,
            5 => ChartType::Area2D,
            6 => ChartType::Step3D,
            7 => ChartType::Step2D,
            8 => ChartType::Combo3D,
            9 => ChartType::Combo2D,
            10 => ChartType::HorizontalBar3D,
            11 => ChartType::HorizontalBar2D,
            12 => ChartType::ClusterBar3D,
            13 => ChartType::Pie3D,
            14 => ChartType::Pie2D,
            15 => ChartType::Doughnut2D,
            16 => ChartType::XY2D,
            17 => ChartType::Polar2D,
            18 => ChartType::Radar2D,
            19 => ChartType::Bubble2D,
            20 => ChartType::HiLo2D,
            21 => ChartType::Gantt2D,
            22 => ChartType::Gantt3D,
            23 => ChartType::Surface3D,
            24 => ChartType::Contour2D,
            25 => ChartType::Scatter3D,
            26 => ChartType::XYZ3D,
            _ => ChartType::Unknown,
        }
    }
}

/// 축 ID 상수 / Axis ID Constants (표 65)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum AxisId {
    /// X 축 / X Axis
    #[default]
    X = 0,
    /// Y 축 / Y Axis
    Y = 1,
    /// 보조 Y 축 / Secondary Y Axis
    SecondaryY = 2,
    /// Z 축 / Z Axis
    Z = 3,
}

impl From<u32> for AxisId {
    fn from(value: u32) -> Self {
        match value {
            0 => AxisId::X,
            1 => AxisId::Y,
            2 => AxisId::SecondaryY,
            3 => AxisId::Z,
            _ => AxisId::X,
        }
    }
}

/// 축 눈금 스타일 상수 / Axis Tick Style Constants (표 66)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum AxisTickStyle {
    /// 눈금 표시가 축에 표시되지 않음 / No tick marks
    #[default]
    None = 0,
    /// 눈금 표시가 축의 가운데 / Tick marks at center
    Center = 1,
    /// 눈금 표시가 축 내부 / Tick marks inside
    Inside = 2,
    /// 눈금 표시가 축 외부 / Tick marks outside
    Outside = 3,
}

impl From<u32> for AxisTickStyle {
    fn from(value: u32) -> Self {
        match value {
            0 => AxisTickStyle::None,
            1 => AxisTickStyle::Center,
            2 => AxisTickStyle::Inside,
            3 => AxisTickStyle::Outside,
            _ => AxisTickStyle::None,
        }
    }
}

/// 브러시 스타일 상수 / Brush Style Constants (표 67)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum BrushStyle {
    /// 브러시 없음 (투명) / No brush (transparent)
    #[default]
    None = 0,
    /// 단색 브러시 / Solid brush
    Solid = 1,
    /// 비트맵 패턴 브러시 / Bitmap pattern brush
    Pattern = 2,
    /// 해치 브러시 / Hatch brush
    Hatch = 3,
}

impl From<u32> for BrushStyle {
    fn from(value: u32) -> Self {
        match value {
            0 => BrushStyle::None,
            1 => BrushStyle::Solid,
            2 => BrushStyle::Pattern,
            3 => BrushStyle::Hatch,
            _ => BrushStyle::None,
        }
    }
}

/// 펜 스타일 상수 / Pen Style Constants (표 92)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum PenStyle {
    /// 펜 없음 / No pen
    #[default]
    None = 0,
    /// 실선 / Solid line
    Solid = 1,
    /// 대시 / Dash
    Dash = 2,
    /// 점선 / Dot
    Dot = 3,
    /// 대시 점 / Dash dot
    DashDot = 4,
    /// 대시 점 점 / Dash dot dot
    DashDotDot = 5,
}

impl From<u32> for PenStyle {
    fn from(value: u32) -> Self {
        match value {
            0 => PenStyle::None,
            1 => PenStyle::Solid,
            2 => PenStyle::Dash,
            3 => PenStyle::Dot,
            4 => PenStyle::DashDot,
            5 => PenStyle::DashDotDot,
            _ => PenStyle::None,
        }
    }
}

/// 채우기 스타일 상수 / Fill Style Constants (표 76)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum FillStyle {
    /// 채우기 없음 (투명) / No fill (transparent)
    #[default]
    None = 0,
    /// 단색 또는 패턴 채우기 / Solid or pattern fill
    Solid = 1,
    /// 그러데이션 채우기 / Gradient fill
    Gradient = 2,
}

impl From<u32> for FillStyle {
    fn from(value: u32) -> Self {
        match value {
            0 => FillStyle::None,
            1 => FillStyle::Solid,
            2 => FillStyle::Gradient,
            _ => FillStyle::None,
        }
    }
}

/// 위치 유형 상수 / Location Type Constants (표 85)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum LocationType {
    /// 위쪽 / Top
    #[default]
    Top = 0,
    /// 왼쪽 맨 위 / Top Left
    TopLeft = 1,
    /// 오른쪽 맨 위 / Top Right
    TopRight = 2,
    /// 왼쪽 / Left
    Left = 3,
    /// 오른쪽 / Right
    Right = 4,
    /// 아래쪽 / Bottom
    Bottom = 5,
    /// 왼쪽 맨 아래 / Bottom Left
    BottomLeft = 6,
    /// 오른쪽 맨 아래 / Bottom Right
    BottomRight = 7,
    /// 사용자 지정 / Custom
    Custom = 8,
}

impl From<u32> for LocationType {
    fn from(value: u32) -> Self {
        match value {
            0 => LocationType::Top,
            1 => LocationType::TopLeft,
            2 => LocationType::TopRight,
            3 => LocationType::Left,
            4 => LocationType::Right,
            5 => LocationType::Bottom,
            6 => LocationType::BottomLeft,
            7 => LocationType::BottomRight,
            8 => LocationType::Custom,
            _ => LocationType::Top,
        }
    }
}

/// 텍스트 방향 상수 / Orientation Constants (표 87)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum Orientation {
    /// 가로 / Horizontal
    #[default]
    Horizontal = 0,
    /// 위에서 아래로 / Top to bottom (stacked)
    TopToBottom = 1,
    /// 아래에서 위로 회전 / Rotated bottom to top
    BottomUp = 2,
    /// 위에서 아래로 회전 / Rotated top to bottom
    TopDown = 3,
}

impl From<u32> for Orientation {
    fn from(value: u32) -> Self {
        match value {
            0 => Orientation::Horizontal,
            1 => Orientation::TopToBottom,
            2 => Orientation::BottomUp,
            3 => Orientation::TopDown,
            _ => Orientation::Horizontal,
        }
    }
}

// ============================================================================
// Basic Types
// ============================================================================

/// RGB 색상 / RGB Color (VtColor Object - 표 2)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VtColor {
    /// 빨간색 구성 요소 / Red component (0-255)
    pub red: u8,
    /// 녹색 구성 요소 / Green component (0-255)
    pub green: u8,
    /// 파란색 구성 요소 / Blue component (0-255)
    pub blue: u8,
    /// 자동 색상 여부 / Automatic color
    pub automatic: bool,
}

impl VtColor {
    /// COLORREF (0x00BBGGRR) 값에서 생성 / Create from COLORREF value
    pub fn from_colorref(value: u32) -> Self {
        VtColor {
            red: (value & 0xFF) as u8,
            green: ((value >> 8) & 0xFF) as u8,
            blue: ((value >> 16) & 0xFF) as u8,
            automatic: false,
        }
    }

    /// COLORREF 값으로 변환 / Convert to COLORREF value
    pub fn to_colorref(&self) -> u32 {
        (self.red as u32) | ((self.green as u32) << 8) | ((self.blue as u32) << 16)
    }
}

/// 좌표 쌍 / Coordinate pair (Coor Object - 표 17)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct Coor {
    /// X 좌표 / X coordinate
    pub x: f32,
    /// Y 좌표 / Y coordinate
    pub y: f32,
}

/// 사각형 / Rectangle (Rect Object - 표 48)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct Rect {
    /// 시작 모서리 / Starting corner
    pub min: Coor,
    /// 끝 모서리 / Ending corner
    pub max: Coor,
}

// ============================================================================
// Chart Objects (스펙 페이지 4-23)
// ============================================================================

/// 펜 객체 / Pen Object (표 42)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Pen {
    /// 펜 스타일 / Pen style
    pub style: PenStyle,
    /// 펜 너비 (포인트) / Pen width (points)
    pub width: f32,
    /// 펜 색상 / Pen color
    pub color: VtColor,
}

/// 브러시 객체 / Brush Object (표 13)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Brush {
    /// 브러시 스타일 / Brush style
    pub style: BrushStyle,
    /// 채우기 색상 / Fill color
    pub fill_color: VtColor,
    /// 패턴 색상 / Pattern color
    pub pattern_color: VtColor,
    /// 패턴/해치 인덱스 / Pattern/Hatch index
    pub index: u32,
}

/// 그러데이션 객체 / Gradient Object (표 29)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Gradient {
    /// 시작 색상 / From color
    pub from_color: VtColor,
    /// 끝 색상 / To color
    pub to_color: VtColor,
    /// 그러데이션 스타일 (0=위아래, 1=좌우, 2=사각형, 3=타원)
    pub style: u32,
}

/// 채우기 객체 / Fill Object (표 26)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Fill {
    /// 채우기 스타일 / Fill style
    pub style: FillStyle,
    /// 브러시 (단색/패턴) / Brush (solid/pattern)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brush: Option<Brush>,
    /// 그러데이션 / Gradient
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gradient: Option<Gradient>,
}

/// 프레임 객체 / Frame Object (표 28)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Frame {
    /// 프레임 스타일 (0=없음, 1=단일, 2=이중 등) / Frame style
    pub style: u32,
    /// 프레임 너비 (포인트) / Frame width (points)
    pub width: f32,
    /// 프레임 색상 / Frame color
    pub frame_color: VtColor,
    /// 공간 색상 / Space color
    pub space_color: VtColor,
}

/// 그림자 객체 / Shadow Object (표 53)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Shadow {
    /// 그림자 스타일 (0=없음, 1=배경) / Shadow style
    pub style: u32,
    /// 오프셋 / Offset
    pub offset: Coor,
    /// 브러시 / Brush
    pub brush: Brush,
}

/// 배경 객체 / Backdrop Object (표 11)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Backdrop {
    /// 프레임 / Frame
    pub frame: Frame,
    /// 채우기 / Fill
    pub fill: Fill,
    /// 그림자 / Shadow
    pub shadow: Shadow,
}

/// 글꼴 객체 / VtFont Object (표 3)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VtFont {
    /// 글꼴 이름 / Font name
    pub name: String,
    /// 글꼴 크기 (포인트) / Font size (points)
    pub size: f32,
    /// 글꼴 스타일 (굵게, 기울임 등) / Font style (bold, italic, etc.)
    pub style: u32,
    /// 글꼴 효과 / Font effects
    pub effects: u32,
    /// 글꼴 색상 / Font color
    pub color: VtColor,
}

/// 텍스트 레이아웃 객체 / TextLayout Object (표 56)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TextLayout {
    /// 텍스트 줄바꿈 / Word wrap
    pub word_wrap: bool,
    /// 가로 맞춤 (0=왼쪽, 1=오른쪽, 2=가운데) / Horizontal alignment
    pub horz_alignment: u32,
    /// 세로 맞춤 (0=위, 1=아래, 2=가운데) / Vertical alignment
    pub vert_alignment: u32,
    /// 방향 / Orientation
    pub orientation: Orientation,
}

/// 위치 객체 / Location Object (표 39)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Location {
    /// 위치 유형 / Location type
    pub location_type: LocationType,
    /// 사각형 좌표 / Rectangle coordinates
    pub rect: Rect,
    /// 표시 여부 / Visibility
    pub visible: bool,
}

/// 제목 객체 / Title Object (표 58)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Title {
    /// 배경 / Backdrop
    pub backdrop: Backdrop,
    /// 위치 / Location
    pub location: Location,
    /// 제목 텍스트 / Title text
    pub text: String,
    /// 텍스트 레이아웃 / Text layout
    pub text_layout: TextLayout,
    /// 글꼴 / Font
    pub font: VtFont,
}

/// 각주 객체 / Footnote Object (표 27)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Footnote {
    /// 배경 / Backdrop
    pub backdrop: Backdrop,
    /// 위치 / Location
    pub location: Location,
    /// 각주 텍스트 / Footnote text
    pub text: String,
    /// 텍스트 레이아웃 / Text layout
    pub text_layout: TextLayout,
    /// 글꼴 / Font
    pub font: VtFont,
}

/// 범례 객체 / Legend Object (표 35)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Legend {
    /// 배경 / Backdrop
    pub backdrop: Backdrop,
    /// 위치 / Location
    pub location: Location,
    /// 텍스트 레이아웃 / Text layout
    pub text_layout: TextLayout,
    /// 글꼴 / Font
    pub font: VtFont,
}

/// 축 눈금 객체 / Tick Object (표 57)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Tick {
    /// 눈금 길이 (포인트) / Tick length (points)
    pub length: f32,
    /// 눈금 스타일 / Tick style
    pub style: AxisTickStyle,
}

/// 축 격자 객체 / AxisGrid Object (표 8)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AxisGrid {
    /// 주 격자선 펜 / Major grid pen
    pub major_pen: Pen,
    /// 부 격자선 펜 / Minor grid pen
    pub minor_pen: Pen,
}

/// 축 배율 객체 / AxisScale Object (표 9)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AxisScale {
    /// 숨김 여부 / Hide axis
    pub hide: bool,
    /// 로그 베이스 / Log base
    pub log_base: u32,
    /// 배율 유형 (0=선형, 1=로그, 2=백분율) / Scale type
    pub scale_type: u32,
}

/// 값 축 배율 객체 / ValueScale Object (표 59)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ValueScale {
    /// 자동 배율 / Auto scale
    pub auto: bool,
    /// 주 분할 수 / Major divisions
    pub major_division: u32,
    /// 부 분할 수 / Minor divisions
    pub minor_division: u32,
    /// 최대값 / Maximum value
    pub maximum: f64,
    /// 최소값 / Minimum value
    pub minimum: f64,
}

/// 축 제목 객체 / AxisTitle Object (표 10)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AxisTitle {
    /// 배경 / Backdrop
    pub backdrop: Backdrop,
    /// 제목 텍스트 / Title text
    pub text: String,
    /// 텍스트 레이아웃 / Text layout
    pub text_layout: TextLayout,
    /// 글꼴 / Font
    pub font: VtFont,
    /// 표시 여부 / Visibility
    pub visible: bool,
}

/// 축 레이블 객체 / Label Object (표 33)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AxisLabel {
    /// 자동 회전 / Auto rotation
    pub auto: bool,
    /// 배경 / Backdrop
    pub backdrop: Backdrop,
    /// 포맷 문자열 / Format string
    pub format: String,
    /// 세움 (3D) / Standing (3D)
    pub standing: bool,
    /// 텍스트 레이아웃 / Text layout
    pub text_layout: TextLayout,
    /// 글꼴 / Font
    pub font: VtFont,
}

/// 축 교차점 객체 / Intersection Object (표 31)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Intersection {
    /// 자동 / Auto
    pub auto: bool,
    /// 교차 축 ID / Crossing axis ID
    pub axis_id: AxisId,
    /// 교차 축 인덱스 / Crossing axis index
    pub index: u32,
    /// 레이블을 플롯 안에 유지 / Keep labels inside plot
    pub labels_inside_plot: bool,
    /// 교차점 / Intersection point
    pub point: f64,
}

/// 축 객체 / Axis Object (표 7)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Axis {
    /// 축 격자 / Axis grid
    pub grid: AxisGrid,
    /// 축 배율 / Axis scale
    pub scale: AxisScale,
    /// 축 제목 / Axis title
    pub title: AxisTitle,
    /// 값 배율 / Value scale
    pub value_scale: ValueScale,
    /// 레이블 / Labels
    pub labels: Vec<AxisLabel>,
    /// 레이블 레벨 수 / Label level count
    pub label_level_count: u32,
    /// 펜 / Pen
    pub pen: Pen,
    /// 눈금 / Tick
    pub tick: Tick,
    /// 교차점 / Intersection
    pub intersection: Intersection,
}

/// 데이터 요소 레이블 객체 / DataPointLabel Object (표 22)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DataPointLabel {
    /// 배경 / Backdrop
    pub backdrop: Backdrop,
    /// 구성 요소 (0=값, 1=백분율, 2=계열이름, 3=요소이름) / Component
    pub component: u32,
    /// 사용자 정의 여부 / Custom
    pub custom: bool,
    /// 선 스타일 / Line style
    pub line_style: u32,
    /// 위치 유형 / Location type
    pub location_type: u32,
    /// 오프셋 / Offset
    pub offset: Coor,
    /// 텍스트 / Text
    pub text: String,
    /// 텍스트 레이아웃 / Text layout
    pub text_layout: TextLayout,
    /// 글꼴 / Font
    pub font: VtFont,
    /// 백분율 포맷 / Percent format
    pub percent_format: String,
    /// 값 포맷 / Value format
    pub value_format: String,
}

/// 마커 객체 / Marker Object (표 41)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Marker {
    /// 채우기 색상 / Fill color
    pub fill_color: VtColor,
    /// 펜 / Pen
    pub pen: Pen,
    /// 크기 (포인트) / Size (points)
    pub size: f32,
    /// 마커 스타일 / Marker style
    pub style: u32,
    /// 표시 여부 / Visibility
    pub visible: bool,
}

/// 데이터 요소 객체 / DataPoint Object (표 21)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DataPoint {
    /// 브러시 / Brush
    pub brush: Brush,
    /// 레이블 / Label
    pub label: DataPointLabel,
    /// 가장자리 펜 / Edge pen
    pub edge_pen: Pen,
    /// 오프셋 / Offset
    pub offset: f32,
    /// 마커 / Marker
    pub marker: Marker,
}

/// 계열 레이블 객체 / SeriesLabel Object (표 51)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SeriesLabel {
    /// 배경 / Backdrop
    pub backdrop: Backdrop,
    /// 선 스타일 / Line style
    pub line_style: u32,
    /// 위치 유형 / Location type
    pub location_type: LocationType,
    /// 오프셋 / Offset
    pub offset: Coor,
    /// 텍스트 / Text
    pub text: String,
    /// 텍스트 레이아웃 / Text layout
    pub text_layout: TextLayout,
    /// 글꼴 / Font
    pub font: VtFont,
}

/// 계열 마커 객체 / SeriesMarker Object (표 52)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SeriesMarker {
    /// 자동 / Auto
    pub auto: bool,
    /// 표시 여부 / Show
    pub show: bool,
}

/// 통계선 객체 / StatLine Object (표 54)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StatLine {
    /// 플래그 (최소, 최대, 평균, 표준편차, 추세) / Flags
    pub flags: u32,
    /// 펜 스타일 / Pen style
    pub style: PenStyle,
    /// 색상 / Color
    pub color: VtColor,
    /// 너비 / Width
    pub width: f32,
}

/// 계열 위치 객체 / Position Object (표 46)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Position {
    /// 제외 여부 / Excluded
    pub excluded: bool,
    /// 숨김 여부 / Hidden
    pub hidden: bool,
    /// 순서 / Order
    pub order: u32,
    /// 스택 순서 / Stack order
    pub stack_order: u32,
}

/// 계열 객체 / Series Object (표 50)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Series {
    /// 데이터 포인트들 / Data points
    pub data_points: Vec<DataPoint>,
    /// 가이드라인 펜 / Guideline pen
    pub guideline_pen: Pen,
    /// HiLo 데이터 / HiLo data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hilo: Option<HiLo>,
    /// 범례 텍스트 / Legend text
    pub legend_text: String,
    /// 펜 / Pen
    pub pen: Pen,
    /// 위치 / Position
    pub position: Position,
    /// 보조 축 사용 / Secondary axis
    pub secondary_axis: bool,
    /// 계열 레이블 / Series label
    pub label: SeriesLabel,
    /// 계열 마커 / Series marker
    pub marker: SeriesMarker,
    /// 계열 유형 / Series type
    pub series_type: u32,
    /// 가이드라인 표시 / Show guide lines
    pub show_guide_lines: bool,
    /// 선 표시 / Show line
    pub show_line: bool,
    /// 다듬기 요소 / Smoothing factor
    pub smoothing_factor: u32,
    /// 다듬기 유형 / Smoothing type
    pub smoothing_type: u32,
    /// 통계선 / Stat line
    pub stat_line: StatLine,
}

/// HiLo 객체 / HiLo Object (표 30)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HiLo {
    /// 상승 색상 / Gain color
    pub gain_color: VtColor,
    /// 하락 색상 / Loss color
    pub loss_color: VtColor,
}

/// 3D 막대 객체 / Bar Object (표 12)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Bar {
    /// 측면 수 / Number of sides
    pub sides: u32,
    /// 상단 비율 / Top ratio
    pub top_ratio: f32,
}

/// 파이 객체 / Pie Object (표 43)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PieObj {
    /// 두께 비율 / Thickness ratio
    pub thickness_ratio: f32,
    /// 상단 반지름 비율 / Top radius ratio
    pub top_radius_ratio: f32,
}

/// 도넛 객체 / Doughnut Object (표 24)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Doughnut {
    /// 측면 수 / Number of sides
    pub sides: u32,
    /// 내부 비율 / Interior ratio
    pub interior_ratio: f32,
}

/// 3D 뷰 객체 / View3D Object (표 60)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct View3D {
    /// 상승 각도 / Elevation angle
    pub elevation: f32,
    /// 회전 각도 / Rotation angle
    pub rotation: f32,
}

/// 벽면 객체 / Wall Object (표 61)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Wall {
    /// 브러시 / Brush
    pub brush: Brush,
    /// 펜 / Pen
    pub pen: Pen,
    /// 두께 (포인트) / Width (points)
    pub width: f32,
}

/// 플롯 기준 객체 / PlotBase Object (표 45)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PlotBase {
    /// 브러시 / Brush
    pub brush: Brush,
    /// 기준 높이 (포인트) / Base height (points)
    pub base_height: f32,
    /// 펜 / Pen
    pub pen: Pen,
}

/// 광원 객체 / LightSource Object (표 38)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LightSource {
    /// X 좌표 / X coordinate
    pub x: f32,
    /// Y 좌표 / Y coordinate
    pub y: f32,
    /// Z 좌표 / Z coordinate
    pub z: f32,
    /// 강도 / Intensity
    pub intensity: f32,
}

/// 조명 객체 / Light Object (표 36)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Light {
    /// 주변광 강도 / Ambient intensity
    pub ambient_intensity: f32,
    /// 가장자리 강도 / Edge intensity
    pub edge_intensity: f32,
    /// 가장자리 표시 / Edge visible
    pub edge_visible: bool,
    /// 광원들 / Light sources
    pub light_sources: Vec<LightSource>,
}

/// 가중치 객체 / Weighting Object (표 62)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Weighting {
    /// 기준 (0=같은크기, 1=총값, 2=첫열) / Basis
    pub basis: u32,
    /// 스타일 (0=영역, 1=지름) / Style
    pub style: u32,
}

/// 플롯 객체 / Plot Object (표 44)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Plot {
    /// 각도 단위 / Angle unit
    pub angle_unit: u32,
    /// 자동 레이아웃 / Auto layout
    pub auto_layout: bool,
    /// 축들 / Axes
    pub axes: Vec<Axis>,
    /// 배경 / Backdrop
    pub backdrop: Backdrop,
    /// 막대 간격 / Bar gap
    pub bar_gap: f32,
    /// 시계방향 / Clockwise
    pub clockwise: bool,
    /// 행에서 데이터 계열 읽기 / Data series in row
    pub data_series_in_row: bool,
    /// 기본 백분율 기준 / Default percent basis
    pub default_percent_basis: u32,
    /// 깊이/높이 비율 / Depth to height ratio
    pub depth_to_height_ratio: f32,
    /// 도넛 / Doughnut
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doughnut: Option<Doughnut>,
    /// 조명 / Light
    pub light: Light,
    /// 위치 사각형 / Location rect
    pub location_rect: Rect,
    /// 3D 뷰 / View 3D
    pub view_3d: View3D,
    /// 플롯 기준 / Plot base
    pub plot_base: PlotBase,
    /// 벽면 / Wall
    pub wall: Wall,
    /// 시리즈들 / Series
    pub series: Vec<Series>,
    /// 투사 유형 / Projection type
    pub projection: u32,
    /// 시작 각도 / Starting angle
    pub starting_angle: f32,
    /// 파이 / Pie
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pie: Option<PieObj>,
    /// 막대 / Bar
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bar: Option<Bar>,
    /// 가중치 / Weighting
    pub weighting: Weighting,
    /// X 간격 / X gap
    pub x_gap: f32,
    /// Z 간격 / Z gap
    pub z_gap: f32,
    /// 정렬 유형 / Sort type
    pub sort: u32,
    /// 하위 플롯 레이블 위치 / Sub plot label position
    pub sub_plot_label_position: u32,
    /// 균일 축 / Uniform axis
    pub uniform_axis: bool,
    /// 너비/높이 비율 / Width to height ratio
    pub width_to_height_ratio: f32,
}

/// 인쇄 정보 객체 / PrintInformation Object (표 47)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PrintInformation {
    /// 아래 여백 / Bottom margin
    pub bottom_margin: f32,
    /// 가로 가운데 / Center horizontally
    pub center_horizontally: bool,
    /// 세로 가운데 / Center vertically
    pub center_vertically: bool,
    /// 프린터용 레이아웃 / Layout for printer
    pub layout_for_printer: bool,
    /// 왼쪽 여백 / Left margin
    pub left_margin: f32,
    /// 방향 (0=세로, 1=가로) / Orientation
    pub orientation: u32,
    /// 오른쪽 여백 / Right margin
    pub right_margin: f32,
    /// 배율 유형 (0=원본, 1=비율유지, 2=채우기) / Scale type
    pub scale_type: u32,
    /// 위 여백 / Top margin
    pub top_margin: f32,
}

/// 데이터 격자 객체 / DataGrid Object (표 19)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DataGrid {
    /// 열 수 / Column count
    pub column_count: u32,
    /// 행 수 / Row count
    pub row_count: u32,
    /// 열 레이블 수 / Column label count
    pub column_label_count: u32,
    /// 행 레이블 수 / Row label count
    pub row_label_count: u32,
    /// 열 레이블들 / Column labels
    pub column_labels: Vec<String>,
    /// 행 레이블들 / Row labels
    pub row_labels: Vec<String>,
    /// 데이터 값들 (행 우선) / Data values (row major)
    pub data: Vec<f64>,
}

// ============================================================================
// Main VtChart Object
// ============================================================================

/// VtChart 객체 / VtChart Object (표 1)
/// 차트의 메인 컨테이너
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VtChart {
    /// 활성 계열 수 / Active series count
    pub active_series_count: u32,
    /// 디더링 허용 / Allow dithering
    pub allow_dithering: bool,
    /// 동적 회전 허용 / Allow dynamic rotation
    pub allow_dynamic_rotation: bool,
    /// 선택 허용 / Allow selections
    pub allow_selections: bool,
    /// 계열 선택 허용 / Allow series selection
    pub allow_series_selection: bool,
    /// 사용자 변경 허용 / Allow user changes
    pub allow_user_changes: bool,
    /// 자동 증가 / Auto increment
    pub auto_increment: bool,
    /// 배경 / Backdrop
    pub backdrop: Backdrop,
    /// 3D 차트 여부 / Is 3D chart
    pub chart_3d: bool,
    /// 차트 유형 / Chart type
    pub chart_type: ChartType,
    /// 데이터 격자 / Data grid
    pub data_grid: DataGrid,
    /// 그리기 모드 / Draw mode
    pub draw_mode: u32,
    /// 각주 / Footnote
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footnote: Option<Footnote>,
    /// 범례 / Legend
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legend: Option<Legend>,
    /// 플롯 / Plot
    pub plot: Plot,
    /// 인쇄 정보 / Print information
    pub print_info: PrintInformation,
    /// 랜덤 채우기 / Random fill
    pub random_fill: bool,
    /// 다시 페인트 / Repaint
    pub repaint: bool,
    /// 범례 표시 / Show legend
    pub show_legend: bool,
    /// 스택 / Stacking
    pub stacking: bool,
    /// 제목 / Title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<Title>,
    /// 너비 (twip) / Width (twips)
    pub twips_width: u32,
    /// 높이 (twip) / Height (twips)
    pub twips_height: u32,
}

// ============================================================================
// ChartData (최상위 구조체)
// ============================================================================

/// 차트 데이터 / Chart Data
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChartData {
    /// VtChart 객체 / VtChart object
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vt_chart: Option<VtChart>,
    /// Raw 데이터 (파싱 실패 시 폴백) / Raw data (fallback when parsing fails)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub raw_data: Vec<u8>,
}

impl ChartData {
    /// ChartData를 바이트 배열에서 파싱합니다. / Parse ChartData from byte array.
    ///
    /// # Arguments
    /// * `data` - ChartData 데이터 / ChartData data
    ///
    /// # Returns
    /// 파싱된 ChartData 구조체 / Parsed ChartData structure
    ///
    /// # Note
    /// 차트 바이너리 데이터는 ChartObj들이 순차적으로 나열된 구조입니다.
    /// Chart binary data consists of ChartObj elements listed sequentially.
    ///
    /// ChartObj 구조:
    /// - id (long, 4바이트)
    /// - StoredtypeId (long, 4바이트)
    /// - StoredName (char*, Variable Data)
    /// - StoredVersion (int, 4바이트, Variable Data)
    /// - ChartObjData
    pub fn parse(data: &[u8]) -> Result<Self, HwpError> {
        // 데이터가 너무 작으면 파싱 불가
        if data.len() < 8 {
            return Ok(ChartData {
                vt_chart: None,
                raw_data: data.to_vec(),
            });
        }

        let mut offset = 0;
        let mut seen_types: std::collections::HashSet<i32> = std::collections::HashSet::new();

        // 첫 번째 객체가 VtChart여야 함
        match parse_chart_obj(data, &mut offset, &mut seen_types) {
            Ok(Some(vt_chart)) => {
                let remaining = if offset < data.len() {
                    data[offset..].to_vec()
                } else {
                    Vec::new()
                };

                Ok(ChartData {
                    vt_chart: Some(vt_chart),
                    raw_data: remaining,
                })
            }
            Ok(None) | Err(_) => Ok(ChartData {
                vt_chart: None,
                raw_data: data.to_vec(),
            }),
        }
    }

    /// 차트 타입 반환 / Get chart type
    pub fn get_chart_type(&self) -> Option<ChartType> {
        self.vt_chart.as_ref().map(|vt| vt.chart_type)
    }

    /// 차트 제목 반환 / Get chart title
    pub fn get_title(&self) -> Option<&str> {
        self.vt_chart
            .as_ref()
            .and_then(|vt| vt.title.as_ref())
            .map(|t| t.text.as_str())
    }

    /// 모든 시리즈 데이터 반환 / Get all series data
    pub fn get_series(&self) -> Vec<&Series> {
        self.vt_chart
            .as_ref()
            .map(|vt| vt.plot.series.iter().collect())
            .unwrap_or_default()
    }

    /// 데이터 격자 반환 / Get data grid
    pub fn get_data_grid(&self) -> Option<&DataGrid> {
        self.vt_chart.as_ref().map(|vt| &vt.data_grid)
    }
}

/// ChartObj 파싱 / Parse ChartObj
fn parse_chart_obj(
    data: &[u8],
    offset: &mut usize,
    seen_types: &mut std::collections::HashSet<i32>,
) -> Result<Option<VtChart>, HwpError> {
    if *offset + 8 > data.len() {
        return Ok(None);
    }

    // id (long, 4바이트)
    let _id = INT32::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;

    // StoredtypeId (long, 4바이트)
    let stored_type_id = INT32::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;

    // Variable Data (StoredName, StoredVersion) - 동일 타입이 없으면 포함
    let _stored_name: Option<String>;
    let _stored_version: Option<i32>;

    if !seen_types.contains(&stored_type_id) {
        // StoredName 파싱 (null-terminated string)
        let name_start = *offset;
        while *offset < data.len() && data[*offset] != 0 {
            *offset += 1;
        }
        if *offset < data.len() {
            _stored_name = Some(
                String::from_utf8_lossy(&data[name_start..*offset]).to_string()
            );
            *offset += 1; // null terminator
        } else {
            _stored_name = None;
        }

        // StoredVersion (int, 4바이트)
        if *offset + 4 <= data.len() {
            _stored_version = Some(INT32::from_le_bytes([
                data[*offset],
                data[*offset + 1],
                data[*offset + 2],
                data[*offset + 3],
            ]));
            *offset += 4;
        } else {
            _stored_version = None;
        }

        seen_types.insert(stored_type_id);
    } else {
        _stored_name = None;
        _stored_version = None;
    }

    // ChartObjData 파싱 - VtChart의 경우 복잡한 구조
    // 현재는 기본 VtChart만 생성하고 나머지는 raw_data로 처리
    let vt_chart = VtChart {
        chart_type: ChartType::Bar2D,
        ..Default::default()
    };

    Ok(Some(vt_chart))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chart_type_conversion() {
        assert_eq!(ChartType::from(0), ChartType::Bar3D);
        assert_eq!(ChartType::from(1), ChartType::Bar2D);
        assert_eq!(ChartType::from(3), ChartType::Line2D);
        assert_eq!(ChartType::from(14), ChartType::Pie2D);
        assert_eq!(ChartType::from(99), ChartType::Unknown);
    }

    #[test]
    fn test_axis_tick_style_conversion() {
        assert_eq!(AxisTickStyle::from(0), AxisTickStyle::None);
        assert_eq!(AxisTickStyle::from(1), AxisTickStyle::Center);
        assert_eq!(AxisTickStyle::from(2), AxisTickStyle::Inside);
        assert_eq!(AxisTickStyle::from(3), AxisTickStyle::Outside);
    }

    #[test]
    fn test_vt_color_from_colorref() {
        let color = VtColor::from_colorref(0x00FF8040);
        assert_eq!(color.red, 0x40);
        assert_eq!(color.green, 0x80);
        assert_eq!(color.blue, 0xFF);
    }

    #[test]
    fn test_parse_empty_data() {
        let result = ChartData::parse(&[]);
        assert!(result.is_ok());
        let chart = result.unwrap();
        assert!(chart.vt_chart.is_none());
    }

    #[test]
    fn test_parse_small_data() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let result = ChartData::parse(&data);
        assert!(result.is_ok());
        let chart = result.unwrap();
        assert!(chart.vt_chart.is_none());
        assert!(!chart.raw_data.is_empty());
    }

    #[test]
    fn test_default_vt_chart() {
        let vt_chart = VtChart::default();
        assert_eq!(vt_chart.chart_type, ChartType::Bar3D);
        assert!(!vt_chart.chart_3d);
        assert!(vt_chart.title.is_none());
    }
}
