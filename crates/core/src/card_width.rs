const VERY_NARROW_MAX_WIDTH: f32 = 216.;
const NARROW_MAX_WIDTH: f32 = 348.;
const STANDARD_MAX_WIDTH: f32 = 540.;
const AT_LEAST_PREFIX: &str = "atleast:";
const AT_MOST_PREFIX: &str = "atmost:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WidthClass {
    VeryNarrow,
    Narrow,
    Standard,
    Wide,
}

impl WidthClass {
    pub fn from_pixels(width: f32) -> WidthClass {
        if width <= VERY_NARROW_MAX_WIDTH {
            WidthClass::VeryNarrow
        } else if width <= NARROW_MAX_WIDTH {
            WidthClass::Narrow
        } else if width <= STANDARD_MAX_WIDTH {
            WidthClass::Standard
        } else {
            WidthClass::Wide
        }
    }

    fn parse(name: &str) -> Option<WidthClass> {
        match name.trim() {
            "verynarrow" => Some(WidthClass::VeryNarrow),
            "narrow" => Some(WidthClass::Narrow),
            "standard" => Some(WidthClass::Standard),
            "wide" => Some(WidthClass::Wide),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetWidth {
    at_least: Option<WidthClass>,
    at_most: Option<WidthClass>,
}

impl TargetWidth {
    pub fn parse(value: &str) -> Option<TargetWidth> {
        let value = value.trim().to_ascii_lowercase();
        if let Some(name) = value.strip_prefix(AT_LEAST_PREFIX) {
            return Some(TargetWidth {
                at_least: Some(WidthClass::parse(name)?),
                at_most: None,
            });
        }
        if let Some(name) = value.strip_prefix(AT_MOST_PREFIX) {
            return Some(TargetWidth {
                at_least: None,
                at_most: Some(WidthClass::parse(name)?),
            });
        }
        let exact = WidthClass::parse(&value)?;
        Some(TargetWidth {
            at_least: Some(exact),
            at_most: Some(exact),
        })
    }

    pub fn matches(&self, width: WidthClass) -> bool {
        self.at_least.is_none_or(|lowest| width >= lowest)
            && self.at_most.is_none_or(|highest| width <= highest)
    }
}
