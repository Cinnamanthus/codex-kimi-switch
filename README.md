# Codex Kimi Switch

Codex ↔ Kimi 本地适配器。不修改 Codex 源码、不经过 cc-switch：它是一个跑在本机的轻量 HTTP 代理，Codex 通过配置指向它，由它把请求转发给 Kimi（Moonshot），并在转发前完成 Kimi 要求的 schema 适配与鉴权注入。

## 它解决什么问题

Kimi 的 Chat Completions 端点（`https://api.kimi.com/coding/v1`）对 `tools[*].function.parameters` 执行严格的 Moonshot Flavored JSON Schema（MFJS）校验，Codex 内置工具生成的 schema（缺 `type`、`type: ["string","null"]`、`$ref` 旁挂结构关键字等）会被 HTTP 400 拒绝。

本适配器在请求离开本机前做三件事：

1. **Schema 清洗**：把 tool parameters 规范化为 MFJS 兼容形态（仅当上游是 Kimi/Moonshot 时启用）。
2. **鉴权替换**：适配器持有 Kimi API key，转发时自动替换客户端的 `Authorization` 头——Codex 配置里不需要保存真实 key。
3. **配置接管/恢复**：启动时自动把 Codex 配置指向本地适配器，退出或执行 `disable` 时字节级还原。

## 构建

```powershell
cd Codex_kimi_switch   # 项目根目录
cargo build --release
```

产物：`target\release\codex_kimi_switch.exe`

## 快速开始

```powershell
# 启动（默认 run：接管 Codex 配置并启动代理；key 从配置文件读取，无需环境变量）
.\target\release\codex_kimi_switch.exe

# 另开终端正常使用 Codex，流量已经走 Kimi
codex

# 回到适配器窗口按 Ctrl+C：代理停止，Codex 配置自动还原
```

启动成功的日志包含两行状态转换：

```text
codex config takeover active
codex_kimi_switch listening
```

如果没有配置任何 API key，启动时会看到警告日志，适配器退化为透传客户端凭证（不会注入 key）。

## 配置文件

适配器的 key 硬编码在配置文件里，启动即读，不再需要每次设置环境变量。查找顺序（先到先生效）：

1. `codex_kimi_switch.toml`（与 exe 同目录）
2. `%USERPROFILE%\.codex-kimi-switch\config.toml`（持久位置，`cargo clean` 不会清掉）

格式：

```toml
api_key = "sk-kimi-..."                              # 必填（缺失则透传客户端凭证并告警）

# 可选项：
# listen_addr = "127.0.0.1:8787"                     # 本地监听地址
# upstream_base = "https://api.kimi.com/coding/v1"   # Kimi 上游 base
```

接管时（`run`/`enable`），如果适配器持有 key，会额外做两件同步：

- 把该 key 写入**用户级持久环境变量** `KIMI_API_KEY`（经 .NET API，带系统广播，新启动的 Codex Desktop 直接继承）；
- 在激活 provider 上写入 `env_key = "KIMI_API_KEY"`，让 Codex 从该环境变量取凭证。

恢复时（退出/`disable`）两者都会回滚：配置文件字节级还原，环境变量恢复为接管前的值（原本没有则删除）。

## 命令

| 命令 | 行为 |
|---|---|
| `run`（默认，可省略） | 备份并接管 Codex 配置 → 启动代理 → Ctrl+C 退出时自动恢复配置 |
| `enable` | 只改写 Codex 配置，不启动代理 |
| `disable` | 恢复接管前的 Codex 配置并退出（也是崩溃后的兜底手段） |

### `run` 参数

| 参数 | 说明 |
|---|---|
| `--listen-addr <ADDR>` | 本地监听地址，默认 `127.0.0.1:8787` |
| `--upstream-base <URL>` | Kimi 上游 base，默认 `https://api.kimi.com/coding/v1` |
| `--api-key <KEY>` | Kimi API key（优先于 `KIMI_API_KEY` 环境变量）；配置后替换客户端 Authorization |
| `--codex-home <DIR>` | 覆盖 Codex 配置目录（默认 `CODEX_HOME` 或 `~/.codex`） |
| `--no-restore-on-exit` | 退出时保留接管状态，不自动还原 |

`enable` 使用同一组参数，但只有 `--listen-addr`、`--codex-home` 会影响 Codex 配置改写；`--api-key`、`--upstream-base` 属于代理运行时参数，仅在 `run` 中生效。

## 环境变量

| 变量 | 默认 | 说明 |
|---|---|---|
| `KIMI_API_KEY` | （无） | Kimi API key；优先级低于 `--api-key`、高于配置文件 |
| `CODEX_KIMI_LISTEN_ADDR` | `127.0.0.1:8787` | 本地监听地址 |
| `CODEX_KIMI_UPSTREAM_BASE` | `https://api.kimi.com/coding/v1` | 上游 base URL |
| `CODEX_KIMI_SANITIZE_ALWAYS` | 关 | 设为 `1/true/yes/on` 时，即使上游 URL 不含 `kimi`/`moonshot` 也强制 schema 清洗 |
| `RUST_LOG` | `info` | 日志级别 |
| `CODEX_HOME` | `~/.codex` | Codex 配置目录 |

优先级：命令行参数 > 环境变量 > 默认值。

## 接管与恢复机制

- 接管前自动备份：`~/.codex/config.toml.codex-kimi-switch.bak`；若原本没有 `config.toml`，则记录 `config.toml.codex-kimi-switch.missing` 标记。
- 改写只动一处：把**当前激活 provider**（顶层 `model_provider` 指向的那个）的 `base_url` 改为本地适配器地址。provider id、`wire_api`、鉴权字段、模型选择、模型目录、注释全部原样保留，桌面端的 provider/模型解析逻辑完全不受影响。
- 恢复是**字节级还原**（或删除本次新建的文件），多次接管不会覆盖首次备份。
- 进程被强杀/断电也不丢配置：备份文件仍在，重新运行一次 `disable` 即可还原。

## HTTP 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| 任意 | 除 `/health` 外的所有路径 | 去掉 `/v1` 前缀后转发到 `{UPSTREAM_BASE}` 的对应路径（如 `/v1/responses` → `…/responses`，`/v1/models` → `…/models`）；仅对 JSON 请求体做 tools schema 清洗，响应/SSE 原样透传 |
| `GET` | `/health` | 健康检查，返回 `{"ok":true}` |

## 常见问题

**怎么确认流量真的走了适配器？**
看适配器日志是否出现 `codex config takeover active` 和 `codex_kimi_switch listening`，或访问 `http://127.0.0.1:8787/health` 应返回 `{"ok":true}`。

**适配器崩溃了，Codex 配置被改了一半怎么办？**
配置不会被改坏——要么完整接管、要么完整还原。重跑一次 `codex_kimi_switch.exe disable` 即可。

**端口被占用？**
`--listen-addr 127.0.0.1:9090` 或设 `CODEX_KIMI_LISTEN_ADDR` 换端口；接管写入 Codex 配置的 `base_url` 会自动跟随该地址。

**想让 Codex 退出后仍保持 Kimi 配置？**
`codex_kimi_switch run --no-restore-on-exit`，之后用 `disable` 手动还原。注意代理不运行时该配置指向的本机地址不可达。

## 开发与验证

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

测试覆盖：schema 清洗 5 例、转发与鉴权替换 3 例（含 fake upstream 端到端）、配置接管/字节级恢复 3 例。
