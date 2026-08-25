# PortWeave

PortWeave 是一个轻量、现代的 Windows SSH 端口映射管理器。它使用
[Iced](https://github.com/iced-rs/iced) 构建原生界面，直接管理系统 OpenSSH
进程，不内置浏览器运行时，也不保存密码或私钥内容。

![Rust](https://img.shields.io/badge/Rust-1.88%2B-000000?logo=rust)
![Iced](https://img.shields.io/badge/Iced-0.14-2f81f7)
![License](https://img.shields.io/badge/license-MIT-green)

## 功能

- 支持远程转发 `RemoteForward` / `ssh -R`
- 支持本地转发 `LocalForward` / `ssh -L`
- 从当前用户的 `~/.ssh/config` 导入连接，支持递归发现 `Include` 文件
- 支持 `ProxyJump` / `ssh -J`，包括逗号分隔的多级跳板链
- 一键启动、停止单个或全部隧道
- 提供中文主界面、状态提示和系统托盘菜单
- 支持随 Windows 登录自动启动 PortWeave
- PortWeave 启动后自动启动指定隧道
- 关闭窗口后继续在系统托盘运行
- 实时显示 SSH 错误和异常退出状态
- 配置仅保存在本机 JSON 文件中
- 使用 Iced `tiny-skia` 软件渲染，避免引入 WGPU

## 配置示例

下面的 OpenSSH 配置：

```ssh-config
Host example-remote
  HostName 203.0.113.10
  User developer
  Port 2222
  ProxyJump bastion
  RemoteForward 127.0.0.1:7897 127.0.0.1:7897
```

在 PortWeave 中对应为：

| 字段 | 值 |
|---|---|
| 名称 | `example-remote` |
| 服务器 | `203.0.113.10` |
| 用户名 | `developer` |
| SSH 端口 | `2222` |
| 跳板机 | `bastion` |
| 方向 | `远程转发 (-R)` |
| 监听 | `127.0.0.1:7897` |
| 目标 | `127.0.0.1:7897` |

应用实际执行等价于：

```powershell
ssh -N -T -o ExitOnForwardFailure=yes -o BatchMode=yes `
  -o ServerAliveInterval=30 -o ServerAliveCountMax=3 `
  -p 2222 -J bastion `
  -R 127.0.0.1:7897:127.0.0.1:7897 developer@203.0.113.10
```

`203.0.113.10` 属于 RFC 5737 文档专用地址，请在本机界面中填写真实服务器信息；
不要把真实基础设施配置提交到 Git。

## 从 SSH config 导入

在隧道页点击 **导入 SSH 配置**。PortWeave 会读取当前用户的
`~/.ssh/config`，递归发现 `Include` 中的文件，并列出不含通配符的具体 `Host`
别名。选择一个连接后，再填写本地或远程端口映射即可保存。

连接参数通过 `ssh -G -F <config> <alias>` 交给系统 OpenSSH 解析，因此
`HostName`、`User`、`Port` 和 `ProxyJump` 会遵循 OpenSSH 的实际匹配结果。保存时
仍保留 `Host` 别名，使该别名下的 `IdentityFile` 等认证设置继续由 OpenSSH 管理；
导入后若修改了 SSH config 中的用户、端口或跳板链，请重新导入或在 PortWeave 中
同步编辑。

也可以不导入，直接在编辑页的 **跳板机（可选）** 中填写 `bastion`、
`user@bastion:2222`，或 `bastion-1,bastion-2`。PortWeave 将整个值作为一个 `-J`
参数传给 OpenSSH，不经过 shell 拼接。

## 运行

要求：

- Windows 10/11
- Rust 1.88 或更高版本（仅从源码构建时需要）
- Windows OpenSSH Client，且 `ssh.exe` 可从 `PATH` 找到
- 已配置的 SSH 密钥或 `ssh-agent`

```powershell
git clone https://github.com/yulianjie/ssh-port-mapping.git
cd ssh-port-mapping
cargo run --release
```

首次连接新服务器时，请先在终端中手动运行一次 `ssh user@host -p port`，核对并接受
服务器主机密钥。PortWeave 使用 `BatchMode=yes`，不会弹出密码或主机密钥输入框。

## 开机启动

在 **设置 → 应用行为** 中启用 **登录 Windows 时自动启动 PortWeave**。该选项写入
当前用户的 Windows 启动项，不需要管理员权限，也不会影响其他用户。若同时启用
**启动时隐藏窗口**，PortWeave 会在登录后直接进入系统托盘。

开机启动记录的是当前 `portweave.exe` 的完整路径。便携版解压后请先放到固定目录；
如果之后移动了程序，请在新位置运行 PortWeave，并将开机启动选项关闭后重新开启。
隧道编辑页中的 **PortWeave 启动时自动启动此隧道** 是独立设置，只有勾选的隧道
才会在应用启动后建立连接。

## 数据与安全

配置文件位于 Windows 用户配置目录：

```text
%APPDATA%\PortWeave\PortWeave\config\config.json
```

- 不保存密码、口令、私钥内容或 SSH agent 凭证。
- 私钥字段只保存用户选择的本地文件路径。
- SSH 参数通过 `Command` 参数数组传递，不经过 shell 拼接。
- SSH config 导入只保存连接字段和本地私钥路径引用，不复制私钥内容。
- 默认绑定地址为 `127.0.0.1`，避免无意暴露到所有网络接口。
- 远程转发是否能绑定非回环地址仍受服务器 `GatewayPorts` 设置约束。
- 退出应用时会停止所有由本次 PortWeave 会话创建的 SSH 子进程。

## 开发与验证

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release
```

项目使用 `default-features = false` 引入 Iced，并仅启用 `tiny-skia`、`tokio` 和
`crisp`。可以使用下面的命令确认依赖树中没有 `wgpu`：

```powershell
cargo tree | Select-String wgpu
```

在 Windows x86_64、Rust 1.95 的一次 0.1.0 release 空闲首屏实测中，二进制为
3.16 MiB，工作集为 24.20 MiB，私有内存为 8.77 MiB。该数字是开发机快照，不是对
所有系统、字体缓存或已运行隧道数量的保证。

## 当前范围

0.3 版本聚焦 Windows 桌面使用。核心配置与 SSH 进程管理代码保持平台无关；macOS
已启用托盘依赖但尚未完成发布验收，Linux 托盘和窗口后端暂未打包。

## License

[MIT](LICENSE)
