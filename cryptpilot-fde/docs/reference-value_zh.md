# 参考值（Reference Value）使用指南

本文档介绍如何在不同启动模式（GRUB 和 UKI）下生成参考值、理解参考值含义，以及将参考值导入 Trustee 服务。

## 什么是参考值

参考值（Reference Value）是可信执行环境（TEE）在启动过程中对系统组件进行度量（measurement）得到的哈希值。这些值用于：

- **完整性验证**：确认系统组件未被篡改
- **远程证明**：向远程验证者证明系统状态
- **安全启动**：确保只有受信任的系统才能启动

## 参考值计算

### 计算命令

使用 `cryptpilot-fde show-reference-value` 命令计算加密镜像的参考值：

```sh
cryptpilot-fde show-reference-value --disk ./encrypted.qcow2
```

### GRUB 模式参考值

GRUB 模式使用传统的引导加载程序，参考值包含多个组件：

```json
{
  "kernel_cmdline": [
    "grub_kernel_cmdline /vmlinuz-5.10.134...",
    "grub_kernel_cmdline (hd0,gpt3)/boot/vmlinuz-5.10.134..."
  ],
  "measurement.kernel_cmdline.SHA-384": [
    "749727fcda2c85df2a901f438da9196233f532cd19fb256f7243acfa930280a2fd9418320bcf87cc8e556fcd7988238a",
    "eb8dfc74e60ede2a3ece37f735db47ece9688458fe3d07ae6a87e0e7e03bf68aabaac534cecc0391af5244fe45336257"
  ],
  "measurement.kernel.SHA-384": [
    "fd4099ae8fdd986173d0fdbe5b06537d8b24f3ed2dc0804407d2061a4e0a9dd73f79a2dae36e34ddc580b919af15f173"
  ],
  "measurement.initrd.SHA-384": [
    "c9ffc25975c0aacf507402bb4c75344b53c005a027620cf774ef93845edc416a5b20b48213b6d4fe3a54f3ca8b7cb4f2"
  ],
  "measurement.grub.SHA-384": [
    "1c6b41cc5f1e08dff906e381580dc5c200b3c4785f3910682c74fd2ac0421f324216165478595b5e799d2b2134d22b75"
  ],
  "measurement.shim.SHA-384": [
    "06647f7cd6b1f00433713e895077c986641bfb6bdd3de989575b4fdc34fe557f26990c414158c772393a27732f959dc5"
  ]
}
```

**GRUB 模式参考值字段说明：**

| 字段 | 说明 |
|------|------|
| `kernel_cmdline` | 内核启动参数（明文） |
| `measurement.kernel_cmdline.SHA-384` | 内核启动参数的 SHA-384 哈希值 |
| `measurement.kernel.SHA-384` | 内核镜像的 SHA-384 哈希值 |
| `measurement.initrd.SHA-384` | initrd 镜像的 SHA-384 哈希值 |
| `measurement.grub.SHA-384` | GRUB 引导程序的 SHA-384 哈希值 |
| `measurement.shim.SHA-384` | Shim（安全启动代理）的 SHA-384 哈希值 |

### UKI 模式参考值

UKI（Unified Kernel Image）模式将内核、initrd 和启动参数打包为单个 EFI 可执行文件，参考值更简洁：

```json
{
  "measurement.uki.SHA-384": [
    "a46e162a57e072be7f660e65504477c646acf6b3bfea4ffc0e3a8ee4f2c2726c2284c8bf1ec2b3bd95b204fe7f4e899c"
  ]
}
```

**UKI 模式参考值字段说明：**

| 字段 | 说明 |
|------|------|
| `measurement.uki.SHA-384` | UKI 文件的 SHA-384 哈希值（包含内核、initrd、cmdline） |

## 导入参考值到 Trustee

### 准备工作

安装 Trustee RVPS 工具：

```sh
# 从 Trustee 仓库下载 rvps-tool
wget https://github.com/confidential-containers/trustee/releases/download/v0.10.0/rvps-tool
chmod +x rvps-tool
```

### 导入参考值到 Trustee

无论是 GRUB 还是 UKI 模式，都可以一次性导入整个参考值文件：

```sh
# 读取参考值文件并编码
REFERENCE_FILE="./reference-value.json"
provenance=$(cat $REFERENCE_FILE | base64 --wrap=0)

# 创建注册请求
cat << EOF > ./register-request.json
{
    "version": "0.1.0",
    "type": "sample",
    "payload": "$provenance"
}
EOF

# 导入到 RVPS
rvps-tool register --path ./register-request.json
```

## 验证参考值

导入后，可以验证参考值是否正确：

```sh
# 列出所有已注册的参考值
rvps-tool list

# 查询特定参考值
rvps-tool query --name "measurement.uki.SHA-384"
```

## 常见问题

### Q: 为什么 UKI 模式只有一个参考值？

A: UKI 将内核、initrd 和启动参数打包为单个 EFI 可执行文件，启动时作为一个整体进行度量，因此只有一个度量值。

### Q: 可以只验证部分参考值吗？

A: 不建议。GRUB 模式下有多个组件，虽然可以修改参考值`reference-value.json`文件仅导入想要的参考值，但建议所有组件都验证，缺少任何一个都会降低安全性。UKI 模式下只有一个度量值，必须验证。

## 参考

- [Trustee RVPS 文档](https://github.com/confidential-containers/trustee/tree/main/rvps)
- [快速开始指南](quick-start_zh.md)
- [启动过程详解](boot_zh.md)
