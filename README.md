# Codex Kimi Switch

Codex ↔ Kimi 本地适配器。不修改 Codex 源码、不经过 cc-switch：它是一个跑在本机的轻量 HTTP 代理，Codex 通过配置指向它，由它把请求转发给 Kimi（Moonshot），并在转发前完成 Kimi 要求的 schema 适配与鉴权注入。

## 它解决什么问题

Kimi 的 Chat Completions / Responses 端点（`https://api.kimi.com/coding/v1`）对 `tools[*].parameters` 执行严格的 Moonshot Flavored JSON Schema（MFJS）校验，且只接受 `type = "function"` 的工具。Codex 内置工具生成的 schema（缺 `type`、`type: ["string","null"]`、`$ref` 旁挂结构关键字）和 `tool_search` 等工具类型会被 HTTP 400 拒绝。

本适配器在请求离开本机前做三件事：

1. **工具过滤与 schema 清洗**：丢弃 Moonshot 不支持的工具类型（`tool_search`/`custom`/`namespace` 等），把存活工具的 parameters 规范化为 MFJS 兼容形态。
2. **鉴权替换**：适配器持有 Kimi API key，转发时自动替换客户端的 `Authorization` 头——Codex 配置里不需要保存真实 key。
3. **配置接管/恢复**：启动时自动把 Codex 激活 provider 的 `base_url` 指向本地适配器，退出或执行 `disable` 时字节级还原。

## 构建

```powershell
cd Codex_kimi_switch   # 项目根目录
cargo build --release
```

产物：`target\release\codex_kimi_switch.exe`

---

## 首次使用

按顺序执行，一步都不能跳：

**1. 构建**（见上节）。

**2. 写入 API key。** 把配置示例 `config.example.toml` 复制到下面这个位置并填入你的 key：

```text
%USERPROFILE%\.codex-kimi-switch\config.toml
```

内容：

```toml
api_key = "sk-kimi-..."   # 必填；缺失则适配器退化为透传模式并在启动时告警

# 可选项（默认值如下，一般不用改）：
# listen_addr = "127.0.0.1:8787"
# upstream_base = "https://api.kimi.com/coding/v1"
```

**3. 完全退出 Codex Desktop**（托盘图标也要退出，不只是关窗口）。桌面端只在启动时读配置和环境变量，跳过这步接管不生效。

**4. 启动适配器**：

```powershell
.\target\release\codex_kimi_switch.exe
```

**5. 核对启动日志。** 正常应看到：

```text
codex config takeover active
codex_kimi_switch listening
```

如果看到 `no Kimi API key configured` 的警告，说明配置文件没放对位置或写错了——适配器会退化为透传模式，不会注入 key。

**6. 启动 Codex Desktop，正常使用。** 此时 Codex 激活 provider 的 `base_url` 已指向 `http://127.0.0.1:8787/v1`，流量经适配器清洗后到达 Kimi。

## 更换 API key（覆写）

key 只存在于一个地方：配置文件 `%USERPROFILE%\.codex-kimi-switch\config.toml`。换 key 的流程：

1. **编辑配置文件**，把 `api_key = "..."` 改成新 key。
2. **重启适配器**（key 只在启动时读取；先关掉旧进程，再启动）。启动时适配器会自动把新 key 同步到用户级环境变量 `KIMI_API_KEY`。
3. **重启 Codex Desktop**（让它继承新的环境变量）。

注意事项：

- **不要相信环境变量里的旧值。** 优先级是 `--api-key` > 配置文件 > 环境变量，配置文件永远说了算；环境变量只是接管时由适配器同步给 Codex 用的下游产物。
- 改错语法（比如少个引号）时适配器会在启动日志里告警并忽略该文件，不会静默用错配置。
- 忘了重启适配器 = 适配器还在用旧 key（它已读进内存）；忘了重启 Codex Desktop = Codex 还在用旧环境变量。两者都要重启。

## 关闭适配器并恢复 Codex 初始配置

三种方式，效果相同（配置 + 环境变量一并回滚）：

**方式一：正常退出（推荐）。** 在适配器窗口按 `Ctrl+C`。代理停止，Codex 配置自动还原。

**方式二：主动关闭。** 适配器不在前台运行时（比如当时是隐藏启动的），另开终端：

```powershell
.\target\release\codex_kimi_switch.exe disable
```

**方式三：崩溃/断电兜底。** 适配器被强杀或机器断电不会损坏配置——备份文件仍在，重跑一次 `disable` 即可还原。

**恢复内容**（字节级，和接管前完全一致）：

- `~/.codex/config.toml`：激活 provider 的 `base_url` 指回原上游、接管时加的 `env_key` 行消失，其余内容（包括注释）分毫不动。
- 用户级环境变量 `KIMI_API_KEY`：恢复为接管前的值；接管前不存在则删除。

**验证已恢复**（可选）：

```powershell
Select-String -Path "$env:USERPROFILE\.codex\config.toml" -Pattern 'base_url'
# 应显示 base_url = "https://api.kimi.com/coding/v1"，且不再有 codex-kimi-switch 的备份文件
```

**想退出适配器但保留接管状态**：`codex_kimi_switch.exe run --no-restore-on-exit`。注意代理不运行时该配置指向的本机地址不可达，之后用 `disable` 手动还原。

---

## 命令

| 命令 | 行为 |
|---|---|
| `run`（默认，可省略） | 备份并接管 Codex 配置 → 启动代理 → Ctrl+C 退出时自动恢复配置 |
| `enable` | 只改写 Codex 配置，不启动代理 |
| `disable` | 恢复接管前的 Codex 配置和环境变量并退出（也是崩溃后的兜底手段） |

### `run` 参数

| 参数 | 说明 |
|---|---|
| `--listen-addr <ADDR>` | 本地监听地址，默认 `127.0.0.1:8787` |
| `--upstream-base <URL>` | Kimi 上游 base，默认 `https://api.kimi.com/coding/v1` |
| `--api-key <KEY>` | Kimi API key（最高优先级，临时覆盖配置文件） |
| `--codex-home <DIR>` | 覆盖 Codex 配置目录（默认 `CODEX_HOME` 或 `~/.codex`） |
| `--no-restore-on-exit` | 退出时保留接管状态，不自动还原 |

`enable` 使用同一组参数，但只有 `--listen-addr`、`--codex-home` 会影响 Codex 配置改写；`--api-key`、`--upstream-base` 属于代理运行时参数，仅在 `run` 中生效。

## 环境变量

| 变量 | 默认 | 说明 |
|---|---|---|
| `KIMI_API_KEY` | （无） | Kimi API key；**优先级最低**，仅在没有配置文件时生效 |
| `CODEX_KIMI_LISTEN_ADDR` | `127.0.0.1:8787` | 本地监听地址 |
| `CODEX_KIMI_UPSTREAM_BASE` | `https://api.kimi.com/coding/v1` | 上游 base URL |
| `CODEX_KIMI_SANITIZE_ALWAYS` | 关 | 设为 `1/true/yes/on` 时，即使上游 URL 不含 `kimi`/`moonshot` 也强制清洗 |
| `RUST_LOG` | `info` | 日志级别 |
| `CODEX_HOME` | `~/.codex` | Codex 配置目录 |

优先级：`--api-key` 等命令行参数 > 配置文件 > 环境变量 > 默认值。

## 接管与恢复机制

- 接管前自动备份：`~/.codex/config.toml.codex-kimi-switch.bak`；若原本没有 `config.toml`，则记录 `config.toml.codex-kimi-switch.missing` 标记。接管时同步环境变量前，旧的环境变量值备份在 `config.toml.codex-kimi-switch.envbak`。
- 改写只动一处：把**当前激活 provider**（顶层 `model_provider` 指向的那个）的 `base_url` 改为本地适配器地址；持有 key 时额外加一行 `env_key = "KIMI_API_KEY"`。provider id、`wire_api`、鉴权字段、模型选择、模型目录、注释全部原样保留，桌面端的 provider/模型解析逻辑完全不受影响。
- 恢复是**字节级还原**（或删除本次新建的文件），多次接管不会覆盖首次备份。
- 进程被强杀/断电也不丢配置：备份文件仍在，重新运行一次 `disable` 即可还原。

## HTTP 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| 任意 | 除 `/health` 外的所有路径 | 去掉 `/v1` 前缀后转发到 `{UPSTREAM_BASE}` 的对应路径（如 `/v1/responses` → `…/responses`，`/v1/models` → `…/models`）；仅对 JSON 请求体做工具过滤与 schema 清洗，响应/SSE 原样透传 |
| `GET` | `/health` | 健康检查，返回 `{"ok":true}` |

## 常见问题

**怎么确认流量真的走了适配器？**
看适配器日志是否出现 `codex config takeover active` 和 `codex_kimi_switch listening`，或访问 `http://127.0.0.1:8787/health` 应返回 `{"ok":true}`。

**重启后报 401 "API Key appears to be invalid"？**
几乎可以肯定是 key 来源不对：检查配置文件里的 key 是否最新（它优先级最高），并确认适配器和 Codex Desktop 都已重启。环境变量里的旧值不会生效，但会迷惑排查——以配置文件为准。

**适配器崩溃了，Codex 配置被改了一半怎么办？**
配置不会被改坏——要么完整接管、要么完整还原。重跑一次 `codex_kimi_switch.exe disable` 即可。

**端口被占用？**
`--listen-addr 127.0.0.1:9090` 或设 `CODEX_KIMI_LISTEN_ADDR` 换端口；接管写入 Codex 配置的 `base_url` 会自动跟随该地址。

**Codex 提示找不到某些插件工具？**
Moonshot 只支持 `function` 类型工具，`tool_search` 等按需加载机制会被适配器丢弃，相关插件能力在 Kimi 会话里不可用；核心工具（shell、文件读写等）不受影响。适配器日志会记录被丢弃的工具类型。

## 开发与验证

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

测试覆盖 16 例：schema 清洗与工具过滤 7 例、转发与鉴权替换 4 例（含 fake upstream 端到端）、配置接管/字节级恢复 4 例、持久环境变量读写 1 例。
