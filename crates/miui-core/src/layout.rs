//! レイアウト制約。Flutter 風の「制約は下へ、サイズは上へ」モデル。

use crate::geometry::Size;

/// 親から子へ渡される最小 / 最大サイズ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    pub min: Size,
    pub max: Size,
}

impl BoxConstraints {
    pub fn new(min: Size, max: Size) -> Self {
        Self { min, max }
    }

    /// 0 以上 `max` 以下。
    pub fn loose(max: Size) -> Self {
        Self {
            min: Size::ZERO,
            max,
        }
    }

    /// サイズを固定する。
    pub fn tight(size: Size) -> Self {
        Self {
            min: size,
            max: size,
        }
    }

    pub const UNBOUNDED: BoxConstraints = BoxConstraints {
        min: Size::ZERO,
        max: Size::new(f32::INFINITY, f32::INFINITY),
    };

    /// 最小制約を外した制約。
    pub fn loosen(&self) -> Self {
        Self {
            min: Size::ZERO,
            max: self.max,
        }
    }

    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min.width, self.max.width),
            size.height.clamp(self.min.height, self.max.height),
        )
    }

    /// 内側に余白を取ったときの子制約。
    pub fn shrink(&self, dw: f32, dh: f32) -> Self {
        Self {
            min: Size::new((self.min.width - dw).max(0.0), (self.min.height - dh).max(0.0)),
            max: Size::new((self.max.width - dw).max(0.0), (self.max.height - dh).max(0.0)),
        }
    }

    pub fn with_max_width(&self, w: f32) -> Self {
        Self {
            min: Size::new(self.min.width.min(w), self.min.height),
            max: Size::new(w, self.max.height),
        }
    }

    pub fn with_max_height(&self, h: f32) -> Self {
        Self {
            min: Size::new(self.min.width, self.min.height.min(h)),
            max: Size::new(self.max.width, h),
        }
    }

    pub fn has_bounded_width(&self) -> bool {
        self.max.width.is_finite()
    }

    pub fn has_bounded_height(&self) -> bool {
        self.max.height.is_finite()
    }
}

/// 主軸方向の配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainAxis {
    #[default]
    Start,
    Center,
    End,
    /// 余白を子の間に均等配分。
    SpaceBetween,
    /// 余白を子の周囲に均等配分。
    SpaceAround,
}

/// 交差軸方向の配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossAxis {
    Start,
    #[default]
    Center,
    End,
    /// 交差軸いっぱいに引き伸ばす。
    Stretch,
}

/// 矩形内の 2 次元配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    #[default]
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Alignment {
    /// (x, y) それぞれ 0.0..=1.0 の係数。
    pub fn factors(self) -> (f32, f32) {
        match self {
            Alignment::TopLeft => (0.0, 0.0),
            Alignment::TopCenter => (0.5, 0.0),
            Alignment::TopRight => (1.0, 0.0),
            Alignment::CenterLeft => (0.0, 0.5),
            Alignment::Center => (0.5, 0.5),
            Alignment::CenterRight => (1.0, 0.5),
            Alignment::BottomLeft => (0.0, 1.0),
            Alignment::BottomCenter => (0.5, 1.0),
            Alignment::BottomRight => (1.0, 1.0),
        }
    }
}
