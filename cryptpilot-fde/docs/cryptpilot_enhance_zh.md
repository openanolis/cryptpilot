# cryptpilot-enhance

## 名称

cryptpilot-enhance — 虚拟机镜像加密前安全加固脚本

## 概要

```bash
cryptpilot-enhance --mode MODE --image IMAGE_PATH [--ssh-key PUBKEY_FILE]
```

## 描述

`cryptpilot-enhance` 是一个用于在磁盘加密前对虚拟机镜像（如 QCOW2 格式）进行安全加固的工具。该脚本基于 `virt-customize` 实现离线修改，无需启动客户机操作系统即可完成系统级安全配置。

所有操作在单次 `virt-customize` 会话中执行，显著降低在 TCG 模式下的性能开销，适用于 CI/CD 流水线、安全构建环境和合规性准备场景。

## 选项

`--mode MODE`  
    设置加固等级。支持两种模式：  
    - `full`：完全加固。移除 SSH 服务，实施最严格访问控制。  
    - `partial`：部分加固。保留 SSH 服务但强制密钥认证，适用于需远程维护的生产环境。

`--image IMAGE_PATH`  
    指定待加固的磁盘镜像路径（支持 QCOW2 或 RAW 格式）。文件必须存在且可读。

`--ssh-key PUBKEY_FILE`  
    （可选）指定 OpenSSH 公钥文件路径。仅在 `partial` 模式下生效，用于向 `root` 用户注入登录密钥。

`--help`  
    显示使用帮助并退出。

## 加固内容

### 通用加固项（两种模式均执行）

- 卸载阿里云助手（Cloud Assistant）：
  - 停止 `aliyun.service` 和 `assist_daemon`
  - 删除相关二进制文件与服务配置
- 卸载安骑士（Aegis）：
  - 下载并执行官方卸载脚本
- 禁用 `rpcbind` 服务：
  - 停止、禁用并屏蔽 `rpcbind.service` 与 `rpcbind.socket`
- 移除 `cloud-init`：
  - 执行 `yum remove -y cloud-init`
- 用户账户清理：
  - 锁定 `root` 和 `admin` 账户密码（在 `/etc/shadow` 中设为 `!!`）
  - 删除除 `root`、`admin` 外所有具有密码的交互式用户账号
  - 清理用户主目录下以 `.DEL` 结尾的临时目录
- 清除 Bash 历史记录：
  - 执行 `history -c && history -w`，防止命令泄露

### 模式特有操作

**`full` 模式**  
- 彻底移除 SSH 服务：`yum remove -y openssh-server`

**`partial` 模式**  
- SSH 安全配置强化：
  - 禁用密码登录：`PasswordAuthentication no`
  - 启用密钥认证：`PubkeyAuthentication yes`
  - 限制 root 登录方式：`PermitRootLogin prohibit-password`
  - 禁用高风险功能：`X11Forwarding no`、`AllowTcpForwarding no`
- 若提供公钥，则注入至 `root` 用户的 `~/.ssh/authorized_keys`

## 示例

对镜像执行完全加固：

```bash
./cryptpilot-enhance \
  --mode full \
  --image ./os-disk.qcow2
```

对镜像执行部分加固并注入 SSH 公钥：

```bash
./cryptpilot-enhance \
  --mode partial \
  --image ./server-disk.qcow2 \
  --ssh-key ~/.ssh/id_rsa.pub
```

## 依赖要求

- 必须安装 `libguestfs-tools` 软件包
- 系统中需可用 `virt-customize` 命令
- 当前用户需具备读写镜像文件的权限

适用于主流 CentOS/RHEL 7/8/9 镜像环境。其他发行版可能需要适配路径或包管理器命令。

默认情况下，`virt-customize` 使用 libvirt 后端，这需要 libvirtd 守护进程正在运行。如果您遇到如下错误：

```
libvirt: XML-RPC error : Failed to connect socket to '/var/run/libvirt/libvirt-sock': No such file or directory
virt-customize: error: libguestfs error: could not connect to libvirt (URI = qemu:///system): Failed to connect socket to '/var/run/libvirt/libvirt-sock': No such file or directory
```

可以通过设置环境变量 `LIBGUESTFS_BACKEND=direct` 来解决此问题：

```bash
LIBGUESTFS_BACKEND=direct ./cryptpilot-enhance --mode partial --image ./server-disk.qcow2
```

## 安全提示

- 本脚本将永久修改磁盘镜像内容。
- 请务必在原始镜像的副本上进行测试。
- 加固后可能失去常规登录能力，请确保已有可信运维通道（如串口、审计网关等）。

## 参见

- `virt-customize(1)`
- `libguestfs-tools(1)`

## 许可证

Apache 许可证。详见项目根目录 LICENSE 文件。