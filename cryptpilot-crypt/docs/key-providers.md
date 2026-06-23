# Key Providers

cryptpilot supports multiple key provider types through modular design. Key providers determine how encryption keys are obtained and managed for encrypted volumes.

## Available Key Providers

### OTP: One-Time Password

Special provider that generates a random password on each open. Suitable for temporary/volatile storage.

> [!IMPORTANT]
> OTP volumes are wiped on each open. Data is NOT persistent across reboots.

**Configuration:**

```toml
[encrypt.otp]
```

No additional fields required.

**Use cases:**
- Temporary scratch space
- Swap partitions
- Cache directories
- Any volatile data storage

**Supported by:** cryptpilot-crypt only (not available for FDE rootfs/data volumes)

Template: [otp.toml.template](../../dist/etc/volumes/otp.toml.template)

---

### KBS: Key Broker Service

Fetches keys from [Key Broker Service (KBS)](https://github.com/openanolis/trustee/tree/main/kbs) using Remote Attestation.

**Configuration:**

Two modes are supported (`cdh_type` is optional, defaults to `one-shot`):

**1. One-shot mode (Default)**
Invokes the `confidential-data-hub` binary to fetch keys.

```toml
[encrypt.kbs]
# cdh_type = "one-shot"
kbs_url = "https://kbs.example.com"
key_uri = "kbs:///default/mykey/volume_data0"
# Optional: HTTPS Root CA certificate (PEM format)
# kbs_root_cert = "-----BEGIN CERTIFICATE-----..."
```

**2. Daemon mode**
Connects to a running CDH daemon via ttrpc.

```toml
[encrypt.kbs]
cdh_type = "daemon"
key_uri = "kbs:///default/mykey/volume_data0"
# Optional: Custom socket path
# cdh_socket = "unix:///run/confidential-containers/cdh.sock"
```

**Use cases:**
- Production workloads requiring attestation
- Multi-tenant environments
- Compliance-sensitive data
- Confidential VM boot verification

**Supported by:** cryptpilot-fde, cryptpilot-crypt

Template: [kbs.toml.template](../../dist/etc/volumes/kbs.toml.template)

---

### KMS: Key Management Service

Fetches keys from [Alibaba Cloud KMS](https://yundun.console.aliyun.com/).

Two authentication modes are supported:

#### Mode 1: Client Key (Application Access Point) — Default

Uses a pre-generated client key for authentication.

```toml
[encrypt.kms]
kms_instance_id = "kst-****"
secret_name = "my-secret"
client_key = '{"KeyId":"KAAP.****","PrivateKeyData":"****"}'
client_key_password = "****"
kms_cert_pem = """
-----BEGIN CERTIFICATE-----
...
-----END CERTIFICATE-----
"""
```

#### Mode 2: ECS RAM Role — No Static Credentials

Bind a RAM role to your ECS instance. cryptpilot automatically discovers the region and role name from the instance metadata service (IMDS).

**Minimal configuration (auto-discover everything):**

```toml
[encrypt.kms]
auth_mode = "ecs_ram_role"
kms_instance_id = "kst-****"
secret_name = "my-secret"
# ecs_ram_role_name and region_id are auto-discovered from IMDS
```

**Explicit configuration:**

```toml
[encrypt.kms]
auth_mode = "ecs_ram_role"
kms_instance_id = "kst-****"
secret_name = "my-secret"
ecs_ram_role_name = "MyKmsRamRole"
region_id = "cn-shanghai"
```

> [!NOTE]
> Auto-discovery of `ecs_ram_role_name` requires the ECS instance to have exactly one RAM role bound. If multiple roles are bound, you must specify `ecs_ram_role_name` explicitly.

**Use cases:**
- Cloud-managed key lifecycle
- Centralized key management
- Integration with Alibaba Cloud services
- Zero static credentials on instances (RAM role mode)

**Supported by:** cryptpilot-fde, cryptpilot-crypt

Template: [kms.toml.template](../../dist/etc/volumes/kms.toml.template)

---

### OIDC: KMS with OpenID Connect

Fetches keys from Alibaba Cloud KMS using OIDC authentication.

Allows configuring an external program to provide the OIDC token. cryptpilot executes this program and uses the token to authenticate with KMS.

**Configuration:**

```toml
[encrypt.oidc]
kms_instance_id = "kst-****"
client_key_password_from_kms = "alias/ClientKey_****"

[encrypt.oidc.oidc_token_from_exec]
command = "/usr/bin/get-oidc-token"
args = []
```

**Use cases:**
- Federated identity integration
- No static credentials on instance
- Short-lived token authentication

**Supported by:** cryptpilot-fde, cryptpilot-crypt

Template: [oidc.toml.template](../../dist/etc/volumes/oidc.toml.template)

---

### Exec: Custom Executable

Executes an external program and uses its stdout as the encryption key.

> [!NOTE]
> The program's stdout is used directly as the key without trimming or processing. Ensure there are no extra characters (newlines, spaces, etc).

**Configuration:**

```toml
[encrypt.exec]
command = "echo"
args = ["-n", "MySecretPassword"]
```

**Use cases:**
- Custom key derivation logic
- Integration with proprietary key management
- Testing and development

**Supported by:** cryptpilot-fde, cryptpilot-crypt

Template: [exec.toml.template](../../dist/etc/volumes/exec.toml.template)

> [!WARNING]
> The exec provider is mainly for testing. Use KBS, KMS, or OIDC in production.

---

## Provider Comparison

| Provider | Attestation | Cloud-Native | Hardware-Bound | Persistent | Use Case |
|----------|-------------|--------------|----------------|------------|----------|
| **OTP** | ❌ | ❌ | ❌ | ❌ | Temporary/volatile storage |
| **KBS** | ✅ | ✅ | ❌ | ✅ | Production with attestation |
| **KMS** | ❌ | ✅ | ❌ | ✅ | Cloud key management |
| **OIDC** | ❌ | ✅ | ❌ | ✅ | Federated identity |
| **Exec** | ❌ | ❌ | ❌ | ✅ | Testing/custom logic |

## See Also

- [FDE Configuration Guide](../../cryptpilot-fde/docs/configuration.md) - Full disk encryption configuration
- [Volume Configuration Guide](configuration.md) - Data volume encryption configuration
- [Development Guide](../../development.md) - Build and test instructions
