# 密钥提供者

cryptpilot 通过模块化设计支持多种密钥提供者类型。密钥提供者决定了如何获取和管理加密卷的加密密钥。

## 可用的密钥提供者

### OTP：一次性密码

特殊提供者，每次打开时生成随机密码。适用于临时/易失性存储。

> [!IMPORTANT]
> OTP 卷每次打开时都会被擦除。数据不会在重启后保留。

**配置：**

```toml
[encrypt.otp]
```

无需额外字段。

**使用场景：**
- 临时暂存空间
- 交换分区
- 缓存目录
- 任何易失性数据存储

**支持范围：** 仅 cryptpilot-crypt（FDE 的 rootfs/data 卷不可用）

模板：[otp.toml.template](../dist/etc/volumes/otp.toml.template)

---

### KBS：Key Broker Service

从 [Key Broker Service (KBS)](https://github.com/openanolis/trustee/tree/main/kbs) 获取密钥，使用远程证明进行认证。

**配置：**

支持以下两种运行模式（`cdh_type` 字段可选，默认为 `one-shot`）：

**1. One-shot 模式 (默认)**
调用 `confidential-data-hub` 命令行工具获取密钥。

```toml
[encrypt.kbs]
# cdh_type = "one-shot"
kbs_url = "https://kbs.example.com"
key_uri = "kbs:///default/mykey/volume_data0"
# 可选：HTTPS 根证书（PEM 格式）
# kbs_root_cert = "-----BEGIN CERTIFICATE-----..."
```

**2. Daemon 模式**
通过 ttrpc 接口连接后台运行的 CDH 守护进程。

```toml
[encrypt.kbs]
cdh_type = "daemon"
key_uri = "kbs:///default/mykey/volume_data0"
# 可选：自定义 socket 路径
# cdh_socket = "unix:///run/confidential-containers/cdh.sock"
```

**使用场景：**
- 需要证明的生产工作负载
- 多租户环境
- 合规敏感数据
- 机密虚拟机启动验证

**支持范围：** cryptpilot-fde, cryptpilot-crypt

模板：[kbs.toml.template](../dist/etc/volumes/kbs.toml.template)

---

### KMS：密钥管理服务（Access Key）

从[阿里云密钥管理服务 KMS](https://yundun.console.aliyun.com/) 获取密钥，使用 Access Key 进行身份验证。

**配置：**

```toml
[encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
```

**使用场景：**
- 云托管的密钥生命周期
- 集中式密钥管理
- 与阿里云服务集成

**支持范围：** cryptpilot-fde, cryptpilot-crypt

模板：[kms.toml.template](../dist/etc/volumes/kms.toml.template)

---

### OIDC：KMS with OpenID Connect

从阿里云 KMS 获取密钥，使用 OIDC 认证协议进行身份验证。

允许配置一个提供 OIDC token 的外部程序。cryptpilot 执行该程序获得 OIDC token，用于 KMS 认证。

**配置：**

```toml
[encrypt.oidc]
kms_instance_id = "kst-****"
client_key_password_from_kms = "alias/ClientKey_****"

[encrypt.oidc.oidc_token_from_exec]
command = "/usr/bin/get-oidc-token"
args = []
```

**使用场景：**
- 联合身份集成
- 实例上无静态凭证
- 短期令牌认证

**支持范围：** cryptpilot-fde, cryptpilot-crypt

模板：[oidc.toml.template](../dist/etc/volumes/oidc.toml.template)

---

### Exec：自定义可执行程序

执行外部程序，将其标准输出作为加密密钥。

> [!NOTE]
> 该外部程序的标准输出数据将原封不动地被当作解密密钥，期间不会进行裁剪或字符串转换。因此您需要确保没有多余的不可见字符如回车符和空格符。

**配置：**

```toml
[encrypt.exec]
command = "echo"
args = ["-n", "MySecretPassword"]
```

**使用场景：**
- 自定义密钥派生逻辑
- 与专有密钥管理集成
- 测试和开发

**支持范围：** cryptpilot-fde, cryptpilot-crypt

模板：[exec.toml.template](../dist/etc/volumes/exec.toml.template)

> [!WARNING]
> exec 提供者主要用于测试。生产环境请使用 KBS、KMS 或 OIDC。

---

## 提供者对比

| 提供者 | 远程证明 | 云原生 | 硬件绑定 | 持久化 | 使用场景 |
|--------|----------|--------|----------|--------|----------|
| **OTP** | ❌ | ❌ | ❌ | ❌ | 临时/易失性存储 |
| **KBS** | ✅ | ✅ | ❌ | ✅ | 生产环境+证明 |
| **KMS** | ❌ | ✅ | ❌ | ✅ | 云密钥管理 |
| **OIDC** | ❌ | ✅ | ❌ | ✅ | 联合身份 |
| **Exec** | ❌ | ❌ | ❌ | ✅ | 测试/自定义逻辑 |
