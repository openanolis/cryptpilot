# Systemd 服务自动打开卷

cryptpilot-crypt 提供了一个 systemd 服务，可在系统启动时自动解密并打开加密卷。

## 概述

`cryptpilot.service` systemd 单元在 System Manager 阶段运行（在 initrd 完成并且系统完全启动后）。它会自动处理所有在配置文件中设置了 `auto_open = true` 的卷。

## 服务详情

- **服务单元**：`cryptpilot.service`
- **位置**：`/usr/lib/systemd/system/cryptpilot.service`
- **执行阶段**：System Manager 阶段（启动后）
- **命令**：`/usr/bin/cryptpilot-crypt boot-service --stage system-volumes-auto-open`

## 工作原理

在系统启动期间，该服务：

1. 扫描 `/etc/cryptpilot/volumes/` 中的所有卷配置文件
2. 识别设置了 `auto_open = true` 的卷
3. 使用配置的密钥提供者尝试打开每个卷
4. 在 `/dev/mapper/<volume-name>` 创建设备映射节点
5. 记录遇到的任何错误

## 启用自动打开

要在启动时自动打开加密卷：

### 1. 配置卷

确保卷配置包含 `auto_open = true`：

```toml
# /etc/cryptpilot/volumes/data0.toml
volume = "data0"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "ext4"

[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/data0-key"
```

### 2. 启用服务

启用并启动 systemd 服务：

```sh
systemctl enable --now cryptpilot.service
```

此命令：
- **`enable`**：配置服务在启动时自动启动
- **`--now`**：立即启动服务（对于当前会话）

### 3. 验证服务状态

检查服务是否正在运行：

```sh
systemctl status cryptpilot.service
```

预期输出：
```
● cryptpilot.service - Auto-open encrypted volumes
     Loaded: loaded (/usr/lib/systemd/system/cryptpilot.service; enabled; vendor preset: disabled)
     Active: active (exited) since ...
```

## 与 /etc/fstab 集成

启用自动打开后，可以在 `/etc/fstab` 中添加条目以实现自动挂载：

```sh
# /etc/fstab
/dev/mapper/data0  /mnt/data0  ext4  defaults  0  2
```

这样可以实现完全自动化的解密和挂载：
1. `cryptpilot.service` 打开加密卷 → `/dev/mapper/data0`
2. `systemd` 根据 `/etc/fstab` 挂载设备 → `/mnt/data0`

## 服务管理

### 启动服务

```sh
systemctl start cryptpilot.service
```

### 停止服务

```sh
systemctl stop cryptpilot.service
```

注意：停止服务不会关闭已经打开的卷。使用 `cryptpilot-crypt close <volume>` 手动关闭卷。

### 重启服务

```sh
systemctl restart cryptpilot.service
```

### 禁用自动启动

要防止在启动时自动打开：

```sh
systemctl disable cryptpilot.service
```

## 参见

- [配置指南](configuration_zh.md) - 卷配置选项
- [主 README](../README.md) - 快速开始和使用示例
- [开发指南](../../docs/development.md) - 构建和测试说明
