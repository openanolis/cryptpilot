# cryptpilot：TEEOS 中操作系统启动和静态数据的机密性保护

[![Building](/../../actions/workflows/build-rpm.yml/badge.svg)](/../../actions/workflows/build-rpm.yml)
![GitHub Release](https://img.shields.io/github/v/release/openanolis/cryptpilot)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

cryptpilot 为机密计算环境提供全面的加密解决方案，保护系统启动完整性和静态数据。

## 项目结构

cryptpilot 分为多个专用软件包：

### [cryptpilot-fde](cryptpilot-fde/)

**全盘加密** - 加密整个系统磁盘并提供启动完整性保护。

- 加密 rootfs 和数据分区
- dm-verity 完整性保护
- 通过远程证明安全获取密钥
- 集成到 initrd 实现早期启动解密

**快速开始：**
```sh
# 加密磁盘镜像
cryptpilot-convert --in ./original.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ --rootfs-passphrase MyPassword
```

📖 [完整文档](cryptpilot-fde/README_zh.md) | [快速开始指南](cryptpilot-fde/docs/quick-start_zh.md)

### [cryptpilot-crypt](cryptpilot-crypt/)

**运行时卷加密** - 在系统运行期间管理加密的数据卷。

- LUKS2 卷加密
- 启动时自动打开
- 多种密钥提供者（KBS、KMS、TPM2 等）
- 使用 dm-integrity 保护完整性

**快速开始：**
```sh
# 初始化并打开卷
cryptpilot-crypt init data0
cryptpilot-crypt open data0
mount /dev/mapper/data0 /mnt/data0
```

📖 [完整文档](cryptpilot-crypt/README_zh.md) | [快速开始指南](cryptpilot-crypt/docs/quick-start_zh.md)

### [cryptpilot-verity](cryptpilot-verity/)

**静态数据度量工具** - 用于计算和验证静态数据的哈希值。

## 功能特性

- **全盘加密**：保护整个系统磁盘，包括 rootfs
- **卷加密**：加密单个数据分区
- **远程证明**：度量并验证启动完整性
- **灵活的密钥管理**：支持 KBS（远程证明）、KMS（阿里云）、OIDC（联合身份）和自定义提供者
- **完整性保护**：支持 dm-verity 和 dm-integrity
- **自动挂载**：启动时自动解密和挂载

## 安装

### 从发布版本安装

从[最新发布版本](https://github.com/openanolis/cryptpilot/releases)下载：

```sh
# 用于全盘加密
rpm --install cryptpilot-fde-*.rpm

# 用于运行时卷加密
rpm --install cryptpilot-crypt-*.rpm

# （可选）主包，用于配置目录
rpm --install cryptpilot-*.rpm
```

### 从源码构建

构建 RPM 包：

```sh
make create-tarball rpm-build
rpm --install /root/rpmbuild/RPMS/x86_64/cryptpilot-*.rpm
```

或构建 DEB 包：

```sh
make create-tarball deb-build
dpkg -i /tmp/cryptpilot_*.deb
```

## 快速示例

### 加密虚拟机磁盘镜像（FDE）

```sh
cryptpilot-convert --in ./source.qcow2 --out ./encrypted.qcow2 \
    -c ./config_dir/ --rootfs-passphrase MyPassword
```

📖 [FDE 详细示例](cryptpilot-fde/docs/quick-start_zh.md)

### 加密数据卷（Crypt）

```sh
cryptpilot-crypt init data0
cryptpilot-crypt open data0
mount /dev/mapper/data0 /mnt/data0
```

📖 [Crypt 详细示例](cryptpilot-crypt/docs/quick-start_zh.md)

## 支持的发行版

- [Anolis OS 23](https://openanolis.cn/anolisos/23)
- [Alibaba Cloud Linux 3](https://www.aliyun.com/product/alinux)

## 文档

### 软件包文档

- [cryptpilot-fde 文档](cryptpilot-fde/README_zh.md)
  - [FDE 配置指南](cryptpilot-fde/docs/configuration_zh.md)
  - [启动过程](cryptpilot-fde/docs/boot_zh.md)
  - [cryptpilot-enhance](cryptpilot-fde/docs/cryptpilot_enhance_zh.md)
  
- [cryptpilot-crypt 文档](cryptpilot-crypt/README_zh.md)
  - [卷配置指南](cryptpilot-crypt/docs/configuration_zh.md)

### 开发

- [开发指南](docs/development.md) - 构建、测试和打包

## 许可证

Apache-2.0

## 贡献

欢迎贡献！请参阅[开发指南](docs/development.md)。

## 参见

- [Trustee 项目](https://github.com/confidential-containers/trustee) - KBS 和证明服务
- [Confidential Containers](https://github.com/confidential-containers) - 云原生机密计算
