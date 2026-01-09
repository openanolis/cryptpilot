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

Template: [otp.toml.template](../dist/etc/volumes/otp.toml.template)

---

### KBS: Key Broker Service

Fetches keys from [Key Broker Service (KBS)](https://github.com/openanolis/trustee/tree/main/kbs) using Remote Attestation.

**Configuration:**

```toml
[encrypt.kbs]
url = "https://kbs.example.com"
resource_path = "/secrets/volume-key"
```

**Use cases:**
- Production workloads requiring attestation
- Multi-tenant environments
- Compliance-sensitive data
- Confidential VM boot verification

**Supported by:** cryptpilot-fde, cryptpilot-crypt

Template: [kbs.toml.template](../dist/etc/volumes/kbs.toml.template)

---

### KMS: Key Management Service (Access Key)

Fetches keys from [Alibaba Cloud KMS](https://yundun.console.aliyun.com/) using Access Key authentication.

**Configuration:**

```toml
[encrypt.kms]
kms_instance_id = "kst-****"
client_key_id = "LTAI****"
client_key_password_from_kms = "alias/ClientKey_****"
```

**Use cases:**
- Cloud-managed key lifecycle
- Centralized key management
- Integration with Alibaba Cloud services

**Supported by:** cryptpilot-fde, cryptpilot-crypt

Template: [kms.toml.template](../dist/etc/volumes/kms.toml.template)

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

Template: [oidc.toml.template](../dist/etc/volumes/oidc.toml.template)

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

Template: [exec.toml.template](../dist/etc/volumes/exec.toml.template)

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

- [FDE Configuration Guide](../cryptpilot-fde/docs/configuration.md) - Full disk encryption configuration
- [Volume Configuration Guide](../cryptpilot-crypt/docs/configuration.md) - Data volume encryption configuration
- [Development Guide](development.md) - Build and test instructions
