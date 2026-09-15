//! 编译期发布渠道标识。
//!
//! 渠道在构建时通过 `GOLD_BAND_RELEASE_CHANNEL` 固定，缺省为 `default`。`src-tauri/build.rs`
//! 负责校验 `configs/channels/<channel>.json` 存在且 `channel` 字段一致；本模块是 core crate
//! 侧的唯一读取点，并提供渠道名常量，避免同一事实在多个模块各自解析或散落字面量。

/// 当前构建的发布渠道。
pub const RELEASE_CHANNEL: &str = match option_env!("GOLD_BAND_RELEASE_CHANNEL") {
    Some(channel) => channel,
    None => "default",
};

/// 内部 wb 渠道名，用于限定仅内部渠道可用的内置能力。
pub const WB_CHANNEL: &str = "wb";
