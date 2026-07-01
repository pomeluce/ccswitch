# CCSwitch

Claude Code 模型配置管理器 — Rust TUI + CLI 工具。

管理多个 API 供应商的模型配置，支持一键切换、代理转发、会话历史浏览。本地模式直接修改 `settings.json`，代理模式通过本地 HTTP 代理自动路由。

## 功能

- **模型管理**：按供应商组织模型配置（DeepSeek, OpenRouter, Z.AI 等），支持增删改
- **一键切换**：本地模式直接写入 `~/.claude/settings.json`，代理模式更新 SQLite
- **会话历史**：自动扫描 Claude Code 本地会话文件，支持搜索、过滤、恢复
- **代理服务**：本地 HTTP 代理（端口 15721），systemd user service / 后台进程
- **Token 用量**：按 profile 统计 Token 消耗，趋势柱状图

## 安装

### Nix

```nix
# Home Manager (推荐)
programs.ccswitch = {
  enable = true;
  defaults = {
    version = 1;
    providers = [
      {
        id = "deepseek";
        name = "DeepSeek";
        api_url = "https://api.deepseek.com/anthropic";
        api_key = "env:DEEPSEEK_API_KEY";
        profiles = [
          {
            id = "v4"; name = "V4";
            opus = "deepseek-v4-pro[1m]";
            sonnet = "deepseek-v4-pro[1m]";
            haiku = "deepseek-v4-flash";
            subagent = "deepseek-v4-flash";
            default = true;
          }
        ];
      }
    ];
  };
};
```

```nix
# NixOS (全局安装)
services.ccswitch.enable = true;
services.ccswitch.defaults = { ... };
```

### Cargo

```bash
cargo install --git https://github.com/your/ccswitch
```

## 使用

### TUI 模式

```bash
ccs    # 无参数启动 TUI
```

按 `1/2/3` 或 `Tab/Shift+Tab` 切换标签页。

### CLI 模式

```bash
# 模型切换
ccs switch deepseek/v4           # 切换到指定的 provider/profile
ccs list                         # 列出所有 provider 和 profile

# 配置管理（仅对用户配置生效，系统默认不可删除/编辑）
ccs add provider                 # 交互式添加供应商
ccs add profile <provider-id>    # 添加模型配置
ccs edit <provider|profile>      # 查看配置
ccs remove <provider|profile>    # 删除用户配置

# 代理服务
ccs proxy start                  # 后台启动代理（自动检测 systemd）
ccs proxy stop                   # 停止代理
ccs proxy status                 # 查看代理状态
ccs proxy serve                  # 前台运行代理（调试用）

# 用量与历史
ccs usage                        # Token 用量统计（默认本周）
ccs usage --day|--week|--month   # 按日/周/月
ccs usage --profile <name>       # 按 profile 过滤
ccs history                      # 会话历史
ccs history --project <name>     # 按项目过滤
ccs history --search <keyword>   # 搜索会话

# Shell 补全 & Man 文档
ccs completions <zsh|bash|fish>  # 生成 Shell 补全脚本
ccs man                          # 输出 roff 格式 man page
```

## 配置

配置文件位置（优先级从高到低）：

- `~/.config/ccswitch/defaults.toml` — XDG 标准（Home Manager 生成）
- `/etc/ccswitch/defaults.toml` — 系统全局默认（NixOS 生成）
- 数据库 `~/.config/ccswitch/ccswitch.db` — 用户配置 + 用量数据

### defaults.toml

```toml
version = 1

[[providers]]
id = "deepseek"
name = "DeepSeek"
api_url = "https://api.deepseek.com/anthropic"
api_key = "env:DEEPSEEK_API_KEY"

[[providers.profiles]]
id = "v4"
name = "V4"
opus = "deepseek-v4-pro[1m]"
sonnet = "deepseek-v4-pro[1m]"
haiku = "deepseek-v4-flash"
subagent = "deepseek-v4-flash"
default = true

[[providers]]
id = "openrouter"
name = "OpenRouter"
api_url = "https://openrouter.ai/api"
api_key = "env:OPENROUTER_API_KEY"

[[providers.profiles]]
id = "claude"
name = "Claude"
opus = "anthropic/claude-opus-4"
sonnet = "anthropic/claude-sonnet-4"
haiku = "anthropic/claude-haiku-4"
subagent = "anthropic/claude-haiku-4"
```

### API Key 格式

| 格式           | 说明                          |
| -------------- | ----------------------------- |
| `env:VAR_NAME` | 从环境变量读取，推荐          |
| `sk-xxx...`    | 直接文本（明文存储，不安全）  |
| 空值           | fallback 到 `$CLAUDE_API_KEY` |

## TUI 快捷键

### 全局

| 键                  | 功能           |
| ------------------- | -------------- |
| `Tab` / `Shift+Tab` | 切换标签页     |
| `1` / `2` / `3`     | 直接跳转标签页 |
| `Q` / `q`           | 退出           |

### 模型标签页

| 键          | 功能                                   |
| ----------- | -------------------------------------- |
| `j/k` `↑/↓` | 上下导航                               |
| `Enter`     | 切换到此模型（弹窗确认）               |
| `D` / `d`   | 删除用户配置（弹窗确认）               |
| `E` / `e`   | 编辑配置（TUI 表单弹窗）               |
| `/`         | 搜索（分词匹配 provider + profile 名） |
| `Esc`       | 退出搜索                               |

### 会话标签页

| 键          | 功能                                   |
| ----------- | -------------------------------------- |
| `j/k` `↑/↓` | 上下导航（循环滚动）                   |
| `Enter`     | 打开会话（弹窗确认，启动 Claude Code） |
| `D` / `d`   | 物理删除会话（弹窗确认）               |
| `/`         | 搜索（分词匹配标题 + 项目名）          |
| `Esc`       | 退出编辑表单 / 关闭弹窗                |

### 用量标签页

| 键          | 功能                     |
| ----------- | ------------------------ |
| `j/k` `↑/↓` | 上下导航                 |
| `t`         | 切换时间范围（天/周/月） |

## 模式

### 本地模式

直接修改 `~/.claude/settings.json` 的 `env` 字段（只更新模型相关变量，保留用户其他配置）。

### 代理模式

1. 启动本地代理监听 `127.0.0.1:15721`
2. 自动设置 `ANTHROPIC_BASE_URL` 指向本地代理
3. 代理根据当前选中的 profile 转发请求到上游 API
4. 切换 profile 时无需重启代理，代理每次请求读取最新配置
5. 支持流式 SSE 响应透传
6. 自动记录 Token 用量到 SQLite

## 开发

```bash
nix develop    # 进入开发环境（Rust 工具链）
cargo build    # 构建
cargo test     # 测试
cargo run      # 启动 TUI
nix build .#   # Nix 打包
```

## License

GPL-3.0

