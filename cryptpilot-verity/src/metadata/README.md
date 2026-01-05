# Metadata Schema Documentation

This directory contains two separate FlatBuffers schema files for cryptpilot-verity metadata management.

## Files Overview

### 1. `metadata.fbs` - Full Metadata Storage Schema

This schema defines the complete metadata structure stored in the `.fb` metadata file.

**Purpose**: Persistent storage of all file verification information.

**Key structures**:
- `Metadata`: Root table containing version and file list
- `FileInfo`: Complete file verification data including:
  - File path
  - Full fs-verity descriptor (version, hash algorithm, block size, data size, root hash, salt)
  - Merkle tree level 1 hashes (binary)
  - Descriptor hash (hex-encoded final measurement)

**Why we need this**:
- Stores all data required to verify file integrity
- Includes redundant fields for quick access and debugging
- Supports version field for backward compatibility
- Contains complete fs-verity implementation details

### 2. `metadata_hash.fbs` - Hash Calculation Schema

This schema defines a minimal structure containing only fields needed for metadata hash calculation.

**Purpose**: Generate deterministic hash of metadata content.

**Key structures**:
- `MetadataHash`: Minimal table for hash calculation
- `FileHashEntry`: Only essential fields:
  - File path
  - Descriptor hash

**Why we need this**:
- **Performance**: Streamlines hash calculation by avoiding full merkle tree data loading
- **Stability**: fs-verity implementation details in metadata.fbs may change over time; separating hash calculation structure ensures existing file hashes remain valid
- **Efficiency**: `descriptor_hash` already cryptographically represents complete file integrity (descriptor + merkle tree), no need to hash redundant data
- **Minimalism**: Only essential fields for verification, sorted by path for deterministic output

## Workflow

```
Format command:
  1. Calculate fs-verity for each file → FileInfo
  2. Serialize full metadata → metadata.fbs → file.fb
  3. Convert to MetadataHash (path + descriptor_hash only)
  4. Serialize MetadataHash → bytes
  5. SHA256(bytes) → root hash

Verify/Open command:
  1. Read metadata file → bytes
  2. Parse to MetadataHash → SHA256 → verify root hash
  3. Parse to Metadata → FileInfo list
  4. Verify individual files
```

## Benefits

1. **Efficiency**: Hash calculation only processes essential data
2. **Clarity**: Clear separation between storage and verification
3. **Flexibility**: Can change storage format without affecting hash
4. **Correctness**: Avoids double-hashing the same cryptographic data
5. **Maintainability**: Two focused schemas are easier to understand than one complex schema with mixed purposes
