# cryptpilot-crypt：运行时卷加密

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

`cryptpilot-crypt` 为机密计算环境提供数据卷的运行时加密。它管理加密的 LUKS2 卷，支持灵活的密钥管理和自动挂载。

## 功能特性

- **卷加密**：使用 LUKS2 加密单个数据卷
- **多种密钥提供者**：KBS、KMS、OIDC、TPM2、Exec、OTP
- **自动打开**：启动时自动解密和挂载卷
- **完整性保护**：可选的 dm-integrity 数据真实性保护
- **灵活的文件系统**：支持 ext4、xfs、vfat、swap

## 加密与完整性

cryptpilot-crypt 使用以下算法进行 LUKS2 卷加密：

- **加密算法**：`aes-xts-plain64`
- **完整性算法**（启用时）：`hmac-sha256`

### 内核配置要求

以下内核配置选项始终需要（用于加密功能）：

```
CONFIG_CRYPTO_AES=y
CONFIG_CRYPTO_AES_NI_INTEL=y
CONFIG_CRYPTO_XTS=y
```

当启用 `integrity = true` 时，还需要以下额外选项：

```
CONFIG_DM_INTEGRITY=y
CONFIG_DM_BUFIO=y
CONFIG_CRYPTO_HMAC=y
CONFIG_AS_SHA256_NI=y
```

## 安装

从[最新发布版本](https://github.com/openanolis/cryptpilot/releases)安装：

```sh
# 安装 cryptpilot-crypt 包
rpm --install cryptpilot-crypt-*.rpm
```

或从源码构建（参见[开发指南](../development.md)）。

## 快速开始

加密数据卷：

```sh
# 创建配置
cat > /etc/cryptpilot/volumes/data0.toml << EOF
volume = "data0"
dev = "/dev/nvme1n1p1"
auto_open = true
makefs = "ext4"

[encrypt.otp]
EOF

# 初始化并打开
cryptpilot-crypt init data0
cryptpilot-crypt open data0
mount /dev/mapper/data0 /mnt/data0
```

📖 [详细快速开始指南](docs/quick-start_zh.md)

## 配置

配置文件位于 `/etc/cryptpilot/volumes/`：

- 每个 `.toml` 文件定义一个卷
- 文件名可以任意（例如 `data0.toml`、`backup.toml`）

详细选项请参阅[配置指南](docs/configuration_zh.md)。

### 配置示例模板

- [otp.toml.template](../dist/etc/volumes/otp.toml.template) - 一次性密码（易失性）
- [kbs.toml.template](../dist/etc/volumes/kbs.toml.template) - 密钥代理服务
- [kms.toml.template](../dist/etc/volumes/kms.toml.template) - 阿里云 KMS
- [oidc.toml.template](../dist/etc/volumes/oidc.toml.template) - 使用 OIDC 的 KMS
- [exec.toml.template](../dist/etc/volumes/exec.toml.template) - 自定义可执行文件

## 命令

### `cryptpilot-crypt show`

显示所有已配置卷的状态：

```sh
cryptpilot-crypt show [卷名称...] [--json]
```

选项：
- `卷名称`：可选的卷名称。如果不指定，则显示所有卷。
- `--json`：以 JSON 格式输出，而非表格格式

示例：
```sh
# 显示所有卷
cryptpilot-crypt show

# 显示指定卷
cryptpilot-crypt show data0
cryptpilot-crypt show data0 data1

# JSON 格式输出
cryptpilot-crypt show --json
cryptpilot-crypt show data0 --json
```

表格输出示例：
```
╭────────┬───────────────────┬─────────────────┬──────────────┬──────────────────┬───────────────╮
│ Volume ┆ Volume Path       ┆ Underlay Device ┆ Key Provider ┆ Extra Options    ┆ Status        │
╞════════╪═══════════════════╪═════════════════╪══════════════╪══════════════════╪═══════════════╡
│ data0  ┆ /dev/mapper/data0 ┆ /dev/nvme1n1p1  ┆ otp          ┆ auto_open = true ┆ ReadyToOpen   │
│        ┆                   ┆                 ┆              ┆ makefs = "ext4"  ┆               │
│        ┆                   ┆                 ┆              ┆ integrity = true ┆               │
╰────────┴───────────────────┴─────────────────┴──────────────┴──────────────────┴───────────────╯
```

JSON 输出示例：
```json
[
  {
    "volume": "data0",
    "volume_path": "/dev/mapper/data0",
    "underlay_device": "/dev/nvme1n1p1",
    "key_provider": "otp",
    "extra_options": {
      "auto_open": true,
      "makefs": "ext4",
      "integrity": true
    },
    "status": "ReadyToOpen",
    "description": "Volume 'data0' uses otp key provider (temporary volume) and is ready to open"
  }
]
```

JSON 输出字段说明：
- `volume`：卷名称
- `volume_path`：解密后的卷路径（始终显示 mapper 路径）
- `underlay_device`：底层加密块设备路径
- `key_provider`：密钥提供者类型（如 `otp`、`kbs`、`kms`、`oidc`、`exec`）
- `extra_options`：额外的卷配置（序列化失败时为 `null`）
- `status`：卷的当前状态（`DeviceNotFound`、`CheckFailed`、`RequiresInit`、`ReadyToOpen`、`Opened`）
- `description`：当前状态的人类可读描述

### `cryptpilot-crypt init`

初始化新的加密卷：

```sh
cryptpilot-crypt init <卷名称>
```


### `cryptpilot-crypt open`

打开（解密）加密卷：

```sh
cryptpilot-crypt open <卷名称>
```

选项：
- `--check-fs`：打开卷后检查文件系统是否已初始化

### `cryptpilot-crypt close`

关闭（卸载并锁定）卷：

```sh
cryptpilot-crypt close <卷名称>
```

### `cryptpilot-crypt config check`

验证卷配置：

```sh
cryptpilot-crypt config check [--keep-checking] [--skip-check-passphrase]
```

选项：
- `--keep-checking`：即使发现错误也继续检查所有卷
- `--skip-check-passphrase`：跳过密码短语验证

## 卷配置选项

每个卷配置支持：

- **`volume`**（必需）：卷名称（用作 `/dev/mapper/<volume>`）
- **`dev`**（必需）：底层块设备路径
- **`auto_open`**（可选，默认：false）：启动时自动解密
- **`makefs`**（可选）：文件系统类型（`ext4`、`xfs`、`vfat`、`swap`）
- **`integrity`**（可选，默认：false）：启用 dm-integrity
- **`encrypt`**（必需）：密钥提供者配置

详情请参阅[配置指南](docs/configuration_zh.md)。

## 密钥提供者

支持多种密钥提供者：

- **OTP**：一次性密码（易失性，每次打开时重新生成）
- **KBS**：带远程证明的密钥代理服务
- **KMS**：使用访问密钥认证的阿里云 KMS
- **OIDC**：使用 OpenID Connect 认证的 KMS
- **Exec**：提供密钥的自定义可执行文件

详细配置请参阅[密钥提供者](docs/key-providers_zh.md)。

## 文档

- [快速开始指南](docs/quick-start_zh.md) - 分步示例
- [配置指南](docs/configuration_zh.md) - 详细配置选项
- [Systemd 服务](docs/systemd-service_zh.md) - 启动时自动打开卷
- [开发指南](../development.md) - 构建和测试说明

## 使用场景

### 临时/易失性存储（OTP）

使用 OTP 提供者实现每次重启后清空的临时空间：

```toml
[encrypt.otp]
```

### 持久化加密存储（KBS）

使用 KBS 实现生产工作负载的证明：

```toml
[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/volume-key"
```

### 云托管密钥（KMS）

使用阿里云 KMS 实现集中式密钥管理：

```toml
[encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
```

## 与 /etc/fstab 集成

打开卷后，添加到 `/etc/fstab` 以实现自动挂载：

```sh
echo "/dev/mapper/data0 /mnt/data0 ext4 defaults 0 2" >> /etc/fstab
```

结合 `auto_open = true`，卷将自动解密和挂载。

## 支持的发行版

- [Anolis OS 23](https://openanolis.cn/anolisos/23)
- [Alibaba Cloud Linux 3](https://www.aliyun.com/product/alinux)

## 许可证

Apache-2.0

## 参见

- [cryptpilot-fde](../cryptpilot-fde/) - 全盘加密
- [cryptpilot-verity](../cryptpilot-verity/) - dm-verity 工具
- [主项目 README](../README_zh.md)
