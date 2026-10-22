// The `missing_docs` lint is declared in Cargo.toml and is useful for the
// library, but this binary crate has nothing meaningful to document beyond
// this comment. Opt out explicitly rather than adding noise.
#![allow(missing_docs)]
//! Auralis Studio 音频引擎入口。
//!
//! 默认模式是**stdio RPC 服务端**：Electron 主进程启动本进程并接管
//! stdin/stdout（协议见 `auralis_engine::rpc`）。stdout 只允许出现 RPC
//! 响应，任何日志都写 stderr。
//!
//! 诊断子命令保留自迁移前的 CLI（ADR-012）：
//!
//! ```text
//! auralis-engine --version               版本与算法版本
//! auralis-engine --self-test             无 GUI 的端到端自检（CI 用）
//! auralis-engine --dump-reference <path> 导出参考数据
//! ```

use std::process::ExitCode;

use auralis_engine::{Engine, init_tracing, rpc};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("auralis-engine {}", auralis_core::APP_VERSION);
        println!("algorithm      {}", auralis_core::ALGORITHM_VERSION);
        println!("dsp spec       {}", auralis_core::DSP_SPEC_REVISION);
        return ExitCode::SUCCESS;
    }

    init_tracing();

    // 这两个分叉（`-> !`）自己结束进程。
    if args.iter().any(|a| a == "--self-test") {
        auralis_engine::run_self_test();
    }

    if let Some(position) = args.iter().position(|a| a == "--dump-reference") {
        match args.get(position + 1) {
            Some(path) => auralis_engine::dump_reference(path),
            None => {
                eprintln!("--dump-reference 需要一个输出路径参数");
                return ExitCode::from(2);
            }
        }
    }

    tracing::info!(
        version = auralis_core::APP_VERSION,
        algorithm = auralis_core::ALGORITHM_VERSION,
        "auralis-engine ready (stdio rpc)"
    );

    rpc::serve(Engine::new())
}
