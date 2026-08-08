use peniko::Color;
use tileink::{TextSubpixelMode, TextWeight};

pub(crate) const WIDTH: u32 = 128;
pub(crate) const HEIGHT: u32 = 34;
pub(crate) const MARGIN: f32 = 6.0;
pub(crate) const FONT_FAMILY: &str = "Segoe UI";
pub(crate) const TEXT: &str = "Catalog path";
pub(crate) const CORE_THRESHOLD: f64 = 0.9;
const PR_SIZES: [u8; 5] = [10, 11, 12, 13, 16];
const DAILY_SIZES: [u8; 6] = [10, 11, 12, 13, 14, 16];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MatrixTier {
    Pr,
    Daily,
    Deep,
}

impl MatrixTier {
    pub(crate) fn from_env() -> Self {
        match std::env::var("TILEINK_TEXT_MATRIX_TIER")
            .unwrap_or_else(|_| "pr".to_owned())
            .to_ascii_lowercase()
            .as_str()
        {
            "pr" => Self::Pr,
            "daily" => Self::Daily,
            "deep" => Self::Deep,
            value => panic!("unknown TILEINK_TEXT_MATRIX_TIER '{value}'; use pr, daily, or deep"),
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Pr => "pr",
            Self::Daily => "daily",
            Self::Deep => "deep",
        }
    }

    pub(crate) const fn thresholds(self) -> MatrixThresholds {
        match self {
            Self::Pr => MatrixThresholds {
                average_core: 0.90..=1.10,
                core_p05_min: 0.62,
                core_p95_max: 1.42,
                ink: 0.95..=1.07,
                average_coverage_mae_max: 0.24,
                coverage_p95_max: 0.33,
                fringe_excess_max: 0.01,
                cohort_core: 0.55..=1.45,
                cohort_ink: 0.90..=1.20,
            },
            Self::Daily => MatrixThresholds {
                average_core: 0.88..=1.12,
                core_p05_min: 0.40,
                core_p95_max: 1.50,
                ink: 0.94..=1.08,
                average_coverage_mae_max: 0.24,
                coverage_p95_max: 0.33,
                fringe_excess_max: 0.01,
                cohort_core: 0.25..=1.80,
                cohort_ink: 0.80..=1.30,
            },
            Self::Deep => MatrixThresholds {
                average_core: 0.85..=1.15,
                core_p05_min: 0.30,
                core_p95_max: 2.00,
                ink: 0.90..=1.10,
                average_coverage_mae_max: 0.27,
                coverage_p95_max: 0.38,
                fringe_excess_max: 0.02,
                cohort_core: 0.15..=2.25,
                cohort_ink: 0.65..=1.40,
            },
        }
    }
}

pub(crate) struct MatrixThresholds {
    pub(crate) average_core: std::ops::RangeInclusive<f64>,
    pub(crate) core_p05_min: f64,
    pub(crate) core_p95_max: f64,
    pub(crate) ink: std::ops::RangeInclusive<f64>,
    pub(crate) average_coverage_mae_max: f64,
    pub(crate) coverage_p95_max: f64,
    pub(crate) fringe_excess_max: f64,
    pub(crate) cohort_core: std::ops::RangeInclusive<f64>,
    pub(crate) cohort_ink: std::ops::RangeInclusive<f64>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PaletteColor {
    pub(crate) name: &'static str,
    pub(crate) rgb: [u8; 3],
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum MatrixWeight {
    Regular,
    Medium,
    Semibold,
}

impl MatrixWeight {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Medium => "medium",
            Self::Semibold => "semibold",
        }
    }

    pub(crate) const fn tileink(self) -> TextWeight {
        match self {
            Self::Regular => TextWeight::NORMAL,
            Self::Medium => TextWeight::MEDIUM,
            Self::Semibold => TextWeight::SEMIBOLD,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct QualityCase {
    pub(crate) foreground_index: usize,
    pub(crate) background_index: usize,
    pub(crate) foreground: PaletteColor,
    pub(crate) background: PaletteColor,
    pub(crate) font_size: u8,
    pub(crate) weight: MatrixWeight,
    pub(crate) opacity_percent: u8,
    pub(crate) phase_thirds: u8,
    pub(crate) subpixel: TextSubpixelMode,
}

impl QualityCase {
    pub(crate) fn id(&self) -> String {
        format!(
            "{}_on_{}_{}px_{}_a{}_p{}_{}",
            self.foreground.name,
            self.background.name,
            self.font_size,
            self.weight.name(),
            self.opacity_percent,
            self.phase_thirds,
            subpixel_name(self.subpixel),
        )
    }

    pub(crate) fn foreground_color(&self) -> Color {
        let [r, g, b] = self.foreground.rgb;
        Color::from_rgba8(
            r,
            g,
            b,
            ((u16::from(self.opacity_percent) * 255 + 50) / 100) as u8,
        )
    }

    pub(crate) fn background_color(&self) -> Color {
        let [r, g, b] = self.background.rgb;
        Color::from_rgb8(r, g, b)
    }

    pub(crate) fn phase(&self) -> f32 {
        f32::from(self.phase_thirds) / 3.0
    }

    pub(crate) fn is_representative_artifact(&self) -> bool {
        self.font_size == 13
            && self.weight == MatrixWeight::Regular
            && self.opacity_percent == 100
            && self.phase_thirds == 0
            && self.subpixel == TextSubpixelMode::Rgb
            && is_pr_pair(self.background_index, self.foreground_index)
    }
}

pub(crate) const fn subpixel_name(mode: TextSubpixelMode) -> &'static str {
    match mode {
        TextSubpixelMode::Rgb => "rgb",
        TextSubpixelMode::Bgr => "bgr",
        TextSubpixelMode::None => "gray",
    }
}

const BACKGROUNDS: [PaletteColor; 12] = [
    PaletteColor {
        name: "white",
        rgb: [255, 255, 255],
    },
    PaletteColor {
        name: "warm_white",
        rgb: [250, 250, 249],
    },
    PaletteColor {
        name: "slate_50",
        rgb: [248, 250, 252],
    },
    PaletteColor {
        name: "gray_100",
        rgb: [243, 244, 246],
    },
    PaletteColor {
        name: "gray_200",
        rgb: [229, 231, 235],
    },
    PaletteColor {
        name: "gray_500",
        rgb: [107, 114, 128],
    },
    PaletteColor {
        name: "zinc_800",
        rgb: [39, 39, 42],
    },
    PaletteColor {
        name: "zinc_900",
        rgb: [24, 24, 27],
    },
    PaletteColor {
        name: "black",
        rgb: [9, 9, 11],
    },
    PaletteColor {
        name: "navy",
        rgb: [15, 23, 42],
    },
    PaletteColor {
        name: "green_dark",
        rgb: [20, 34, 28],
    },
    PaletteColor {
        name: "red_dark",
        rgb: [38, 20, 23],
    },
];

const FOREGROUNDS: [PaletteColor; 24] = [
    PaletteColor {
        name: "near_black",
        rgb: [20, 20, 22],
    },
    PaletteColor {
        name: "slate_700",
        rgb: [51, 65, 85],
    },
    PaletteColor {
        name: "slate_500",
        rgb: [100, 116, 139],
    },
    PaletteColor {
        name: "slate_400",
        rgb: [148, 163, 184],
    },
    PaletteColor {
        name: "zinc_400",
        rgb: [161, 161, 170],
    },
    PaletteColor {
        name: "near_white",
        rgb: [245, 245, 246],
    },
    PaletteColor {
        name: "blue_700",
        rgb: [29, 78, 216],
    },
    PaletteColor {
        name: "blue_600",
        rgb: [37, 99, 235],
    },
    PaletteColor {
        name: "blue_400",
        rgb: [96, 165, 250],
    },
    PaletteColor {
        name: "blue_300",
        rgb: [147, 197, 253],
    },
    PaletteColor {
        name: "green_700",
        rgb: [21, 128, 61],
    },
    PaletteColor {
        name: "green_600",
        rgb: [22, 163, 74],
    },
    PaletteColor {
        name: "green_400",
        rgb: [74, 222, 128],
    },
    PaletteColor {
        name: "red_700",
        rgb: [185, 28, 28],
    },
    PaletteColor {
        name: "red_600",
        rgb: [220, 38, 38],
    },
    PaletteColor {
        name: "red_400",
        rgb: [248, 113, 113],
    },
    PaletteColor {
        name: "amber_700",
        rgb: [180, 83, 9],
    },
    PaletteColor {
        name: "amber_500",
        rgb: [245, 158, 11],
    },
    PaletteColor {
        name: "yellow_400",
        rgb: [250, 204, 21],
    },
    PaletteColor {
        name: "violet_700",
        rgb: [109, 40, 217],
    },
    PaletteColor {
        name: "violet_500",
        rgb: [139, 92, 246],
    },
    PaletteColor {
        name: "purple_300",
        rgb: [216, 180, 254],
    },
    PaletteColor {
        name: "cyan_600",
        rgb: [8, 145, 178],
    },
    PaletteColor {
        name: "cyan_300",
        rgb: [103, 232, 249],
    },
];

pub(crate) fn matrix_cases(tier: MatrixTier) -> Vec<QualityCase> {
    let sizes = match tier {
        MatrixTier::Pr => PR_SIZES.as_slice(),
        MatrixTier::Daily | MatrixTier::Deep => DAILY_SIZES.as_slice(),
    };
    let weights = match tier {
        MatrixTier::Pr => [MatrixWeight::Regular].as_slice(),
        MatrixTier::Daily | MatrixTier::Deep => [
            MatrixWeight::Regular,
            MatrixWeight::Medium,
            MatrixWeight::Semibold,
        ]
        .as_slice(),
    };
    let opacity = match tier {
        MatrixTier::Pr | MatrixTier::Daily => [100].as_slice(),
        MatrixTier::Deep => [50, 75, 100].as_slice(),
    };
    let phases = match tier {
        MatrixTier::Pr | MatrixTier::Daily => [0].as_slice(),
        MatrixTier::Deep => [0, 1, 2].as_slice(),
    };
    let modes = match tier {
        MatrixTier::Pr | MatrixTier::Daily => [TextSubpixelMode::Rgb].as_slice(),
        MatrixTier::Deep => [TextSubpixelMode::Rgb, TextSubpixelMode::Bgr].as_slice(),
    };
    let mut cases = Vec::new();
    for (background_index, &background) in BACKGROUNDS.iter().enumerate() {
        for (foreground_index, &foreground) in FOREGROUNDS.iter().enumerate() {
            if tier == MatrixTier::Pr && !is_pr_pair(background_index, foreground_index) {
                continue;
            }
            for &font_size in sizes {
                for &weight in weights {
                    for &opacity_percent in opacity {
                        for &phase_thirds in phases {
                            for &subpixel in modes {
                                cases.push(QualityCase {
                                    foreground_index,
                                    background_index,
                                    foreground,
                                    background,
                                    font_size,
                                    weight,
                                    opacity_percent,
                                    phase_thirds,
                                    subpixel,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    cases
}

fn is_pr_pair(background_index: usize, foreground_index: usize) -> bool {
    let neutral = if background_index < 6 { 0 } else { 5 };
    let muted = if background_index < 6 { 2 } else { 4 };
    let accent = [7, 11, 14, 17, 20, 22][background_index % 6];
    let second_accent = [14, 17, 20, 22, 7, 11][background_index % 6];
    [neutral, muted, accent, second_accent].contains(&foreground_index)
}
