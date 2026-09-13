// Shared by the Criterion workload and its exact-pixel prerequisite.
pub const CASES: [(&str, &str); 6] = [
    ("diagonal", "shapes/line/simple-case.svg"),
    ("star-clip", "masking/clipPath/simple-case.svg"),
    ("blur", "filters/feDropShadow/only-stdDeviation.svg"),
    ("turbulence", "filters/feTurbulence/numOctaves=5.svg"),
    (
        "lighting",
        "filters/feSpecularLighting/with-fePointLight.svg",
    ),
    (
        "gradient",
        "paint-servers/linearGradient/attributes-via-xlink-href.svg",
    ),
];
pub const WIDTHS: [u32; 2] = [300, 1600];
