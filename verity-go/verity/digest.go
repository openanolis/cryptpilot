// verity-go/verity/digest.go
package verity

import "hash"

// fixedSizeBlock tracks a hash being filled to block_size bytes.
type fixedSizeBlock struct {
	h         hash.Hash
	remaining int
	algo      HashAlgorithm
	salt      []byte
	blockSize int
}

func newFixedSizeBlock(algo HashAlgorithm, salt []byte, blockSize int) fixedSizeBlock {
	return fixedSizeBlock{
		h:         newSaltedHash(algo, salt),
		remaining: blockSize,
		algo:      algo,
		salt:      salt,
		blockSize: blockSize,
	}
}

// newSaltedHash creates a hash.Hash initialized with the salt.
func newSaltedHash(algo HashAlgorithm, salt []byte) hash.Hash {
	h := algo.newHash()
	blockSize := algo.blockSize()
	for i := 0; i < len(salt); i += blockSize {
		end := i + blockSize
		if end > len(salt) {
			end = len(salt)
		}
		h.Write(salt[i:end])
		padding := blockSize - (end - i)
		if padding < blockSize {
			h.Write(make([]byte, padding))
		}
	}
	return h
}

// append writes data to the block.
func (b *fixedSizeBlock) append(data []byte) {
	b.h.Write(data)
	b.remaining -= len(data)
}

// fillToEnd pads with zeros to block_size and returns the hash.
func (b *fixedSizeBlock) fillToEnd() []byte {
	if b.remaining > 0 {
		b.h.Write(make([]byte, b.remaining))
		b.remaining = 0
	}
	return b.h.Sum(nil)
}

// overflowingAppend appends as much as possible, returning the overflow.
func (b *fixedSizeBlock) overflowingAppend(data []byte) []byte {
	n := b.remaining
	if len(data) < n {
		n = len(data)
	}
	b.append(data[:n])
	return data[n:]
}

// finalizeAndReset computes the hash and resets to a fresh salted state.
func (b *fixedSizeBlock) finalizeAndReset() []byte {
	out := b.fillToEnd()
	b.h = newSaltedHash(b.algo, b.salt)
	b.remaining = b.blockSize
	return out
}

// FsVerityConfig holds the parameters for fs-verity hashing.
type FsVerityConfig struct {
	blockSize int
	salt      []byte
	algo      HashAlgorithm
}

// FsVerityDigest is a streaming fs-verity hasher.
type FsVerityDigest struct {
	config     FsVerityConfig
	levels     []fixedSizeBlock
	merkleTree *MerkleTree
}

// NewFsVerity creates a new fs-verity hasher with empty salt.
func NewFsVerity(algo HashAlgorithm) *FsVerityDigest {
	return NewFsVerityWithSaltAndBlockSize(algo, nil, DefaultBlockSize)
}

// NewFsVerityWithSalt creates a new fs-verity hasher with the given salt.
func NewFsVerityWithSalt(algo HashAlgorithm, salt []byte) *FsVerityDigest {
	return NewFsVerityWithSaltAndBlockSize(algo, salt, DefaultBlockSize)
}

// NewFsVerityWithSaltAndBlockSize creates a new fs-verity hasher with custom parameters.
func NewFsVerityWithSaltAndBlockSize(algo HashAlgorithm, salt []byte, blockSize int) *FsVerityDigest {
	return &FsVerityDigest{
		config: FsVerityConfig{
			blockSize: blockSize,
			salt:      salt,
			algo:      algo,
		},
		levels:     make([]fixedSizeBlock, 0),
		merkleTree: &MerkleTree{algo: algo},
	}
}

// Write implements io.Writer.
func (d *FsVerityDigest) Write(data []byte) (int, error) {
	d.update(data)
	return len(data), nil
}

// update processes data in block_size chunks, matching the Rust implementation.
func (d *FsVerityDigest) update(data []byte) {
	digestSize := d.config.algo.digestSize()

	for chunkStart := 0; chunkStart < len(data); chunkStart += d.config.blockSize {
		chunkEnd := chunkStart + d.config.blockSize
		if chunkEnd > len(data) {
			chunkEnd = len(data)
		}
		overflow := data[chunkStart:chunkEnd]

		keepSpace := false
		for levelIdx := range d.levels {
			level := &d.levels[levelIdx]
			overflow = level.overflowingAppend(overflow)

			if keepSpace {
				if level.remaining >= digestSize {
					if len(overflow) == 0 {
						break
					}
					// overflow but not enough room — finalize this level
				}
			} else {
				if len(overflow) == 0 {
					break
				}
			}

			hash := level.finalizeAndReset()
			if levelIdx == 0 {
				d.merkleTree.level1 = append(d.merkleTree.level1, hash)
			}
			overflow = hash
			keepSpace = true
		}

		if len(overflow) > 0 {
			level := newFixedSizeBlock(d.config.algo, d.config.salt, d.config.blockSize)
			level.append(overflow)
			d.levels = append(d.levels, level)
		}
	}
}

// Finalize flushes all levels and returns the descriptor and merkle tree.
func (d *FsVerityDigest) Finalize() (FsVerityDescriptor, *MerkleTree) {
	digestSize := d.config.algo.digestSize()
	compressionFactor := d.config.blockSize / digestSize

	var totalSize int
	scale := 1

	// Flush all levels from bottom to top
	var lastHash []byte
	overflow := lastHash // initially empty (zeros)

	for levelIdx, level := range d.levels {
		totalSize += scale * (d.config.blockSize - level.remaining)
		level.append(overflow)
		lastHash = level.fillToEnd()
		if levelIdx == 0 {
			d.merkleTree.level1 = append(d.merkleTree.level1, lastHash)
		}
		overflow = lastHash
		scale *= compressionFactor
	}

	if lastHash == nil {
		// Empty file: root hash is all zeros
		lastHash = make([]byte, digestSize)
	}

	desc := FsVerityDescriptor{
		Version:       1,
		HashAlgorithm: d.config.algo,
		LogBlocksize:  uint8(trailingZeros(uint(d.config.blockSize))),
		DataSize:      uint64(totalSize),
		RootHash:      lastHash,
		Salt:          append([]byte(nil), d.config.salt...),
	}

	// Return merkleTree reference for caller
	tree := d.merkleTree
	d.merkleTree = &MerkleTree{algo: d.config.algo}

	return desc, tree
}

// InnerHashAlgorithm returns the hash algorithm in use.
func (d *FsVerityDigest) InnerHashAlgorithm() HashAlgorithm {
	return d.config.algo
}

// trailingZeros counts trailing zeros in an unsigned integer.
func trailingZeros(n uint) int {
	if n == 0 {
		return 0
	}
	count := 0
	for n&1 == 0 {
		count++
		n >>= 1
	}
	return count
}
