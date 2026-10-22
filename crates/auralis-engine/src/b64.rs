//! 二进制字段的 JSON 编码。
//!
//! `Vec<u8>` 默认会被 `serde_json` 序列化成**数字数组**：每个字节一个十进制
//! 数字。一个 1024 bin 的频谱会从 4 KiB 膨胀到约 30 KiB 文本，前端还要逐元素
//! 建立数组再转换——这正是 ADR-018 明令禁止的形态。
//!
//! 这里改成 base64 字符串：体积只增加 33%，前端一次解码就还原成
//! `Uint8Array`，仍然满足「二进制优先」。传输层的完整取舍见 `src/rpc.rs`。

use base64::Engine as _;
use serde::Serializer;

/// 把字节切片序列化成标准 base64 字符串。
///
/// # Errors
/// 序列化器本身出错时向上抛出。
pub fn encode<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
}
