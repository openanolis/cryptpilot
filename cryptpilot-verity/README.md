# cryptpilot-verity: Userspace Filesystem Integrity Protection

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

`cryptpilot-verity` is a command-line tool for generating, validating, and using fs-verity style integrity metadata for a read-only data directory. It can be seen as a user-space implementation of fs-verity tailored for generic read-only directory trees.
It computes an integrity "root hash" over the full dataset, stores per-file verification metadata in a FlatBuffers format, and can mount the data directory
through a FUSE filesystem that enforces filesystem-level integrity checks at read time.

## Relationship to dm-verity, fs-verity and composefs

`cryptpilot-verity` is conceptually similar to Linux dm-verity and the in-kernel fs-verity feature, but operates entirely in user space and is focused on directory trees rather than block devices or individual files.

- **Compared to [dm-verity](https://docs.kernel.org/admin-guide/device-mapper/verity.html)**: dm-verity protects a block device at the block layer, while `cryptpilot-verity` protects a read-only directory tree exposed via a FUSE filesystem. There is no requirement to provision or manage a dedicated verity block device.
- **Compared to in-kernel [fs-verity](https://docs.kernel.org/filesystems/fsverity.html)**: fs-verity today only supports a limited set of filesystems (ext4, f2fs, btrfs) and does not apply to user-space filesystems such as object-storage backed FUSE mounts or virtio-fs based shares. fs-verity also works at **per-file granularity** and does **not** protect filesystem metadata (directory entries, links between paths and inodes). An attacker who controls the lower storage can therefore change the directory structure so that upper layers open a different file that does **not** have fs-verity enabled. `cryptpilot-verity` instead measures and verifies the **entire directory tree**, including the mapping from paths to protected files.
- **Compared to [composefs](https://github.com/composefs/composefs)**: composefs focuses on composing immutable filesystem trees from content-addressed objects, primarily for container images. It makes excellent use of existing kernel features: an EROFS image is used to store path and directory metadata, overlayfs combines that EROFS view with an underlying `objects/` directory, and dm-verity can be enabled for the EROFS block device to protect metadata integrity. However, the actual file payloads still rely on the filesystem hosting the object directory (for example ext4) to enable fs-verity. Converting a plain directory into composefs also requires rewriting the layout into an object store and building an EROFS image. In contrast, `cryptpilot-verity` is deliberately lightweight: it does not modify the original files or directory layout, but only adds a FlatBuffers metadata file (`cryptpilot-verity.metadata.fb`) that records the Merkle trees and descriptors used for verification.

The CLI interface and subcommand design are intentionally similar to the `veritysetup` tooling, so that users familiar with dm-verity find it easy to adopt.

## Threat Model

`cryptpilot-verity` is primarily designed for confidential-computing style deployments where a virtual machine mounts a read-only data directory whose backing storage is **not trusted** (for example, a host-side disk, object storage such as OSS, a remote NAS, or a virtio-fs share backed by untrusted storage). An attacker may be able to modify the underlying storage at any time, but cannot directly compromise the guest kernel.

- **What we defend against**:
  - Offline or online tampering with file contents in the protected directory tree.
  - Attacks that try to replace a protected file with an unprotected one by changing the directory structure.
  - Path traversal and symlink tricks that attempt to escape the intended tree or redirect file accesses. The implementation relies on Rust's type system together with kernel features such as `openat2()` + `RESOLVE_BENEATH` to ensure paths stay confined.
  - Runtime read-time tampering: data is re-verified using a Merkle tree before being returned to the caller, very similar to the fs-verity mechanism.

- **What the verity measurement covers**:
  - **File contents** of the protected files.
  - **File paths** and their association with protected content, so that changing which file a path points to is detectable.

- **What is *not* covered**:
  - POSIX metadata such as permissions bits, ownership (`uid`, `gid`), and timestamps.
  - Mount options, kernel-side permission checks, or higher-level application logic.
  - Integrity of files or directories that were never included in the formatted metadata; in practice such paths are ignored and do not appear in the exposed filesystem view. Likewise, if a file that was included in the metadata is later removed from the underlying filesystem, this is treated as absence rather than active tampering and does not by itself trigger an integrity failure.
  - Labels (key-value metadata attached during format). Labels are stored in the metadata file but are not integrity-protected by the root hash.
  
## Security Notes

- The tool assumes the data directory is **read-only** once formatted; modifying underlying files after formatting will cause verification failures.
- The FUSE layer performs filesystem-level verification on read and returns I/O errors if integrity checks fail.
- The integrity of the metadata file itself does not need separate protection: as long as the expected root hash is protected, any tampering with the metadata will be detected when the hash is recomputed.
- Always protect the expected root hash from tampering; it forms the trust anchor for verification and can be safeguarded using mechanisms such as TPM measurements or dynamic attestation inside a confidential-computing TEE.

## Features

- **Format**: Scan a data directory and compute fs-verity descriptors, Merkle trees, and a global root hash.
- **Verify**: Recompute the metadata hash and compare it against an expected root hash.
- **Dump**: Inspect metadata files or print only the root hash for debugging or integration.
- **Open**: Mount a data directory via `verity-fuse` with on-access verification enabled.
- **Close**: Unmount a previously mounted verity-fuse filesystem.

## High-Level Workflow

1. **Format the data directory**
   - Walks the directory tree.
   - Computes fs-verity descriptors and Merkle trees for each file.
   - Stores full metadata (descriptor, Merkle tree, descriptor hash) in a FlatBuffers file.
   - Derives a deterministic metadata hash (root hash) from a minimal view of the metadata.

2. **Verify integrity later**
   - Reads the metadata file and recomputes the metadata hash.
   - Compares the recomputed hash with the expected root hash you provide.

3. **Mount with verification**
   - Uses the metadata to create a `verity-fuse` filesystem.
   - Each read is verified against the Merkle tree before data is returned to the caller.

## Dependencies

- **`fusermount` (or `fusermount3`)**: Required for the `open` and `close` subcommands to mount/unmount the FUSE filesystem.
- **No `libfuse3` needed**: The binary does **not** dynamically link against `libfuse3.so`. It uses a pure Rust FUSE implementation that communicates directly with the kernel via `/dev/fuse`. Only `fusermount` (the user-space mount helper) needs to be installed on the target system.
- All other subcommands (`format`, `verify`, `dump`) have no external dependencies.

## Commands

All commands are subcommands of the `cryptpilot-verity` binary. Run `cryptpilot-verity --help` or `cryptpilot-verity <subcommand> --help` for details.

### `format`

```bash
cryptpilot-verity format <DATA_DIR> [--metadata <METADATA_PATH>] [--force] [--label key=value]... --hash-output <HASH_OUTPUT>
```

- **Purpose**: Generate fs-verity metadata and the root hash for a given data directory.
- **Arguments**:
  - `<DATA_DIR>`: Path to the data directory for which to calculate reference values.
  - `--metadata, -m` **[optional]**: Path to the output metadata file (FlatBuffers-encoded). If not specified, defaults to `<DATA_DIR>/cryptpilot-verity.metadata.fb`.
  - `--hash-output`: Path to write the root hash (use `-` for stdout).
  - `--force` **[optional]**: Overwrite an existing metadata file at the target path. Intended for re-formatting or third-party auditing of an already formatted directory.
  - `--label key=value` **[optional, repeatable]**: Attach a label to the metadata. Labels are key-value pairs (Docker-style) stored in the metadata file. Can be specified multiple times. Labels are NOT included in the root hash calculation.

### `verify`

```bash
cryptpilot-verity verify <DATA_DIR> <HASH> [--metadata <METADATA_PATH>] [--metadata-only]
```

- **Purpose**: Verify that the metadata for a data directory matches an expected root hash.
- **Arguments**:
  - `<DATA_DIR>`: Path to the data directory to verify.
  - `<HASH>`: Expected root hash (hex-encoded).
  - `--metadata, -m` **[optional]**: Path to the metadata file. If not specified, defaults to `<DATA_DIR>/cryptpilot-verity.metadata.fb`.
  - `--metadata-only` **[optional]**: Only verify metadata integrity without reading actual files. When enabled, only checks that the metadata hash matches the expected root hash and validates metadata self-consistency, without verifying individual file contents against their descriptors.

### `dump`

```bash
cryptpilot-verity dump <DATA_DIR> --print-metadata
cryptpilot-verity dump --metadata <METADATA_PATH> --print-root-hash
cryptpilot-verity dump <DATA_DIR> --print-label <KEY>
cryptpilot-verity dump <DATA_DIR> --print-labels
```

- **Purpose**: Inspect metadata and/or print only the root hash.
- **Arguments**:
  - `<DATA_DIR>` **[optional]**: Path to the data directory from which to read metadata. Either `<DATA_DIR>` or `--metadata` must be specified (not both required). If `<DATA_DIR>` is provided without `--metadata`, reads from `<DATA_DIR>/cryptpilot-verity.metadata.fb`.
  - `--metadata` **[optional]**: Path to the metadata file to read directly. Either `--metadata` or `<DATA_DIR>` must be specified (not both required).
  - `--print-metadata`: Print the full decoded metadata (must specify either this or `--print-root-hash`).
  - `--print-root-hash`: Print only the root hash (must specify either this or `--print-metadata`).
  - `--print-label <KEY>`: Print the value of a specific label key. Exits with an error if the key is not found.
  - `--print-labels`: Print all labels (one `key=value` per line). Prints `(no labels)` if no labels were set during format.

### `open`

```bash
cryptpilot-verity open <DATA_DIR> <MOUNT_POINT> <HASH> [--metadata <METADATA_PATH>]
```

- **Purpose**: Mount the data directory as a verity-fuse filesystem with verification enabled.
- **Arguments**:
  - `<DATA_DIR>`: Path to the data directory to mount (must match the metadata).
  - `<MOUNT_POINT>`: Target mount point for the FUSE filesystem.
  - `<HASH>`: Expected root hash; used to validate the metadata before mounting.
  - `--metadata, -m` **[optional]**: Path to the metadata file. If not specified, defaults to `<DATA_DIR>/cryptpilot-verity.metadata.fb`.

### `close`

```bash
cryptpilot-verity close <MOUNT_POINT>
```

- **Purpose**: Unmount a verity-fuse filesystem previously mounted with `open`.
- **Arguments**:
  - `<MOUNT_POINT>`: Mount point to unmount.

## Metadata Format

Metadata is stored and consumed using a FlatBuffers schema defined in `src/metadata/metadata.fbs`. The resulting FlatBuffers file (typically named `cryptpilot-verity.metadata.fb`) is what `cryptpilot-verity` uses for verification and mounting.

The hash algorithm for individual files is fully compatible with the Linux kernel's fs-verity implementation (SHA-256 hash with empty salt and 4096-byte blocks by default). This means that for any given file, the fs-verity descriptor hash computed by `cryptpilot-verity` matches exactly what the kernel's `FS_IOC_ENABLE_VERITY` ioctl would produce with the same parameters, and also matches the output of the `fsverity digest` command from the [fsverity-utils](https://git.kernel.org/pub/scm/fs/fsverity/fsverity-utils.git/) toolset.

The metadata file stores per-file Merkle trees and descriptors. As a rule of thumb, the metadata size is approximately **1/128** of the total data directory size (for example, a 1 GiB data directory typically produces around 8 MiB of metadata). The exact size depends on file count and size distribution, but this ratio holds for typical workloads with files larger than a few blocks.
