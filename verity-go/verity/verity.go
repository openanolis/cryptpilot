// verity-go/verity/verity.go
package verity

import (
	"crypto/sha256"
	"crypto/sha512"
	"hash"
)

const (
	MaxDigestSize    = 64
	MaxSaltSize      = 32
	MaxLevels        = 8 // FS_VERITY_MAX_LEVELS
	DefaultBlockSize = 4096
)

// HashAlgorithm matches kernel FS_VERITY_HASH_ALG_* values.
type HashAlgorithm uint8

const (
	HashSHA256 HashAlgorithm = 1
	HashSHA512 HashAlgorithm = 2
)

func (h HashAlgorithm) String() string {
	switch h {
	case HashSHA256:
		return "sha256"
	case HashSHA512:
		return "sha512"
	default:
		return "unknown"
	}
}

func (h HashAlgorithm) DigestSize() int {
	switch h {
	case HashSHA256:
		return sha256.Size
	case HashSHA512:
		return sha512.Size
	default:
		return 0
	}
}

func (h HashAlgorithm) BlockSize() int {
	switch h {
	case HashSHA256:
		return sha256.BlockSize
	case HashSHA512:
		return sha512.BlockSize
	default:
		return 0
	}
}

func (h HashAlgorithm) newHash() hash.Hash {
	switch h {
	case HashSHA256:
		return sha256.New()
	case HashSHA512:
		return sha512.New()
	default:
		return nil
	}
}

// FsVerityDescriptor is a kernel-compatible fs-verity descriptor.
type FsVerityDescriptor struct {
	Version       uint8
	HashAlgorithm HashAlgorithm
	LogBlocksize  uint8
	DataSize      uint64
	RootHash      []byte // len = digestSize (32 for SHA-256, 64 for SHA-512)
	Salt          []byte // 0..32 bytes
}

// BlockSize returns the Merkle tree block size (1 << LogBlocksize).
func (d *FsVerityDescriptor) BlockSize() int {
	return 1 << d.LogBlocksize
}

// ToDescriptorHash computes the salted hash of the descriptor (kernel format).
// See: https://www.kernel.org/doc/html/latest/filesystems/fsverity.html#fs-verity-descriptor
func (d *FsVerityDescriptor) ToDescriptorHash() []byte {
	h := saltToDigest(d.HashAlgorithm, d.Salt)
	h.Write([]byte{d.Version})
	h.Write([]byte{uint8(d.HashAlgorithm)})
	h.Write([]byte{d.LogBlocksize})
	h.Write([]byte{uint8(len(d.Salt))})
	h.Write(make([]byte, 4)) // __reserved_0x04
	h.Write(d.DataSizeToBytes())
	hashPadded(h, d.RootHash, 64)
	hashPadded(h, d.Salt, 32)
	h.Write(make([]byte, 144)) // __reserved[144]
	return h.Sum(nil)
}

// DataSizeToBytes returns data_size as little-endian bytes.
func (d *FsVerityDescriptor) DataSizeToBytes() []byte {
	b := make([]byte, 8)
	b[0] = byte(d.DataSize)
	b[1] = byte(d.DataSize >> 8)
	b[2] = byte(d.DataSize >> 16)
	b[3] = byte(d.DataSize >> 24)
	b[4] = byte(d.DataSize >> 32)
	b[5] = byte(d.DataSize >> 40)
	b[6] = byte(d.DataSize >> 48)
	b[7] = byte(d.DataSize >> 56)
	return b
}

// saltToDigest creates a hash.Hash initialized with the salt padded to block boundaries.
func saltToDigest(algo HashAlgorithm, salt []byte) hash.Hash {
	h := algo.newHash()
	blockSize := algo.BlockSize()
	for i := 0; i < len(salt); i += blockSize {
		end := i + blockSize
		if end > len(salt) {
			end = len(salt)
		}
		h.Write(salt[i:end])
		// Pad this chunk to block size
		padding := blockSize - (end - i)
		if padding < blockSize {
			h.Write(make([]byte, padding))
		}
	}
	return h
}

// hashPadded writes data to the hash, padding with zeros to the target size.
func hashPadded(h hash.Hash, data []byte, targetSize int) {
	h.Write(data)
	if len(data) < targetSize {
		h.Write(make([]byte, targetSize-len(data)))
	}
}
