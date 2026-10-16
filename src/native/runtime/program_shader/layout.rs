//! Runtime source has its own reflected interface; no fixed filter layout.
use crate::native::shaders::NativeShaderArtifact;
pub(super) static SOURCE: NativeShaderArtifact = NativeShaderArtifact {
    format: "user",
    entry: "native_source",
    cache_key: "native-source-v1",
    workgroup: [1; 3],
    bindings: &[],
    #[cfg(feature = "metal")]
    uniforms: &[],
    bytes: &[],
};
#[cfg(feature = "metal")]
pub(super) static CONFIG: &[crate::native::shaders::UniformLayout] =
    &[crate::native::shaders::UniformLayout {
        slot: 0,
        fields: &[
            (0, 0),
            (4, 0),
            (8, 0),
            (12, 0),
            (16, 0),
            (20, 0),
            (24, 0),
            (28, 0),
            (32, 0),
            (36, 0),
            (40, 0),
            (44, 0),
            (48, 2),
            (52, 2),
            (56, 2),
            (60, 2),
            (64, 2),
            (68, 2),
            (72, 2),
            (76, 2),
            (80, 2),
            (84, 2),
            (88, 2),
            (92, 0),
            (96, 0),
            (100, 0),
            (104, 0),
            (108, 0),
        ],
    }];
