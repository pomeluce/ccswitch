# CCSwitch

Claude Code 模型配置管理器 — Rust TUI + CLI 工具。

快速切换 Claude Code 的模型供应商和模型配置，支持本地模式（直接修改 settings.json）和代理模式（本地 HTTP 代理转发）。

## 安装

### Nix (推荐)

```nix
# flake.nix
inputs.ccswitch.url = "github:your/ccswitch";

# NixOS module
services.ccswitch.enable = true;
services.ccswitch.defaults = {
  version = 1;
  providers = [ ... ];
};
```

### Cargo

```bash
cargo install --git https://github.com/your/ccswitch
```

## 使用

### TUI 模式

```bash
ccs    # 启动交互式 TUI
```

### CLI 模式

```bash
# 模型切换
ccs switch deepseek/v4          # 切换到指定配置
ccs list                        # 列出所有供应商和配置

# 配置管理
ccs add provider                # 交互式添加供应商
ccs add profile <provider>     # 添加模型配置
ccs remove <provider/profile>  # 删除用户配置

# 代理服务
ccs proxy start                 # 启动代理
ccs proxy stop                  # 停止代理
ccs proxy status                # 查看状态
ccs proxy serve                 # 前台运行（调试）

# 用量与历史
ccs usage                       # Token 用量统计
ccs history                     # 会话历史

# 其他
ccs completions zsh             # 生成 zsh 补全
ccs man                         # 输出 man page
```

## 快捷键 (TUI)

| 键 | 功能 |
|----|------|
| `1/2/3` | 切换 Tab |
| `j/k` `↑/↓` | 上下导航 |
| `h/l` `←/→` | 折叠/展开供应商 |
| `Enter` | 选中并应用 |
| `/` | 搜索 |
| `a` | 添加供应商 |
| `A` | 添加配置 |
| `e` | 编辑 |
| `d` | 删除 |
| `p` | 切换本地/代理模式 |
| `s` | 启动/停止代理 |
| `q` | 退出 |

## 配置

### 系统默认配置 (`/etc/ccswitch/defaults.toml`)

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
```

### API Key 格式

- `env:VAR_NAME` — 从环境变量读取
- `sk-xxx...` — 直接使用文本值
- 空值 — fallback 到 `CLAUDE_API_KEY`

## 模式

### 本地模式

直接修改 `~/.claude/settings.json`，Claude Code 读取生效。

### 代理模式

启动本地代理 `127.0.0.1:15721`，Claude Code 请求经过代理转发到上游 API。
切换模型时无需重启 Claude Code，代理自动感知配置变更。
