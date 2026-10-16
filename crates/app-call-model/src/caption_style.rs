#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptionSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl CaptionSize {
    pub const ALL: [CaptionSize; 3] = [CaptionSize::Small, CaptionSize::Medium, CaptionSize::Large];

    pub fn label(self) -> &'static str {
        match self {
            CaptionSize::Small => "Small",
            CaptionSize::Medium => "Medium",
            CaptionSize::Large => "Large",
        }
    }

    pub fn text_size(self) -> f32 {
        match self {
            CaptionSize::Small => 13.,
            CaptionSize::Medium => 16.,
            CaptionSize::Large => 21.,
        }
    }

    pub fn meta(self) -> &'static str {
        match self {
            CaptionSize::Small => "small",
            CaptionSize::Medium => "medium",
            CaptionSize::Large => "large",
        }
    }

    pub fn from_meta(value: &str) -> CaptionSize {
        match value {
            "small" => CaptionSize::Small,
            "large" => CaptionSize::Large,
            _ => CaptionSize::Medium,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptionPosition {
    #[default]
    Bottom,
    Top,
}

impl CaptionPosition {
    pub const ALL: [CaptionPosition; 2] = [CaptionPosition::Bottom, CaptionPosition::Top];

    pub fn label(self) -> &'static str {
        match self {
            CaptionPosition::Bottom => "Bottom",
            CaptionPosition::Top => "Top",
        }
    }

    pub fn meta(self) -> &'static str {
        match self {
            CaptionPosition::Bottom => "bottom",
            CaptionPosition::Top => "top",
        }
    }

    pub fn from_meta(value: &str) -> CaptionPosition {
        match value {
            "top" => CaptionPosition::Top,
            _ => CaptionPosition::Bottom,
        }
    }
}
