// verity-go/metadata/metadata.go
package metadata

import (
	"encoding/hex"
	"fmt"
	"sort"

	"cryptpilot-verity-go/metadata/generated"
	"cryptpilot-verity-go/verity"

	flatbuffers "github.com/google/flatbuffers/go"
)

// FileVerityInfo holds fs-verity data for a single file.
type FileVerityInfo struct {
	Path           string
	Descriptor     verity.FsVerityDescriptor
	MerkleTree     *verity.MerkleTree
	DescriptorHash string // hex-encoded
}

// VerifySelf checks that the descriptor hash matches and the Merkle tree is consistent.
func (f *FileVerityInfo) VerifySelf() error {
	calculated := hex.EncodeToString(f.Descriptor.ToDescriptorHash())
	if calculated != f.DescriptorHash {
		return &VerificationError{
			Path:     f.Path,
			Field:    "descriptor_hash",
			Expected: f.DescriptorHash,
			Got:      calculated,
		}
	}
	rebuiltRoot := f.MerkleTree.RebuildRootHash(f.Descriptor.Salt, f.Descriptor.BlockSize())
	if !equalBytes(rebuiltRoot, f.Descriptor.RootHash) {
		return &VerificationError{
			Path:     f.Path,
			Field:    "root_hash",
			Expected: hex.EncodeToString(f.Descriptor.RootHash),
			Got:      hex.EncodeToString(rebuiltRoot),
		}
	}
	return nil
}

// MetadataInfo holds deserialized metadata with file info and labels.
type MetadataInfo struct {
	FileInfos []FileVerityInfo
	Labels    map[string]string
}

// VerificationError describes an integrity check failure.
type VerificationError struct {
	Path     string
	Field    string
	Expected string
	Got      string
}

func (e *VerificationError) Error() string {
	return fmt.Sprintf("verification failed for %s: %s mismatch, expected %s, got %s",
		e.Path, e.Field, e.Expected, e.Got)
}

// MetadataVersionError is returned when the metadata format version is unsupported.
type MetadataVersionError struct {
	Version uint32
}

func (e *MetadataVersionError) Error() string {
	return fmt.Sprintf("unsupported metadata version: %d, expected version 1", e.Version)
}

// ParseError wraps FlatBuffers parsing failures.
type ParseError struct {
	Message string
}

func (e *ParseError) Error() string {
	return fmt.Sprintf("failed to parse metadata: %s", e.Message)
}

// SerializeMetadata serializes file info and labels to FlatBuffers bytes.
// Files are sorted by path for deterministic output.
func SerializeMetadata(fileInfos []FileVerityInfo, labels map[string]string) ([]byte, error) {
	builder := flatbuffers.NewBuilder(0)

	// Sort by path for stable output
	sorted := make([]*FileVerityInfo, len(fileInfos))
	for i := range fileInfos {
		sorted[i] = &fileInfos[i]
	}
	sort.Slice(sorted, func(i, j int) bool {
		return sorted[i].Path < sorted[j].Path
	})

	fileOffsets := make([]flatbuffers.UOffsetT, len(sorted))
	for i, info := range sorted {
		pathOff := builder.CreateString(info.Path)
		hashOff := builder.CreateString(info.DescriptorHash)

		rootHashVec := builder.CreateByteVector(info.Descriptor.RootHash)
		saltVec := builder.CreateByteVector(info.Descriptor.Salt)

		generated.FsVerityDescriptorStart(builder)
		generated.FsVerityDescriptorAddVersion(builder, info.Descriptor.Version)
		generated.FsVerityDescriptorAddHashAlgorithm(builder, uint8(info.Descriptor.HashAlgorithm))
		generated.FsVerityDescriptorAddLogBlocksize(builder, info.Descriptor.LogBlocksize)
		generated.FsVerityDescriptorAddDataSize(builder, info.Descriptor.DataSize)
		generated.FsVerityDescriptorAddRootHash(builder, rootHashVec)
		generated.FsVerityDescriptorAddSalt(builder, saltVec)
		descOff := generated.FsVerityDescriptorEnd(builder)

		merkleVec := builder.CreateByteVector(info.MerkleTree.Level1AsBytes())

		generated.FileInfoStart(builder)
		generated.FileInfoAddPath(builder, pathOff)
		generated.FileInfoAddDescriptor(builder, descOff)
		generated.FileInfoAddMerkleTreeLevel1(builder, merkleVec)
		generated.FileInfoAddDescriptorHash(builder, hashOff)
		fileOffsets[i] = generated.FileInfoEnd(builder)
	}

	filesVec := builder.CreateVectorOfTables(fileOffsets)

	// Build labels vector (sorted by key)
	labelKeys := make([]string, 0, len(labels))
	for k := range labels {
		labelKeys = append(labelKeys, k)
	}
	sort.Strings(labelKeys)

	labelOffsets := make([]flatbuffers.UOffsetT, len(labelKeys))
	for i, k := range labelKeys {
		keyOff := builder.CreateString(k)
		valOff := builder.CreateString(labels[k])
		generated.KeyValueStart(builder)
		generated.KeyValueAddKey(builder, keyOff)
		generated.KeyValueAddValue(builder, valOff)
		labelOffsets[i] = generated.KeyValueEnd(builder)
	}

	var labelsVec flatbuffers.UOffsetT
	if len(labelOffsets) > 0 {
		labelsVec = builder.CreateVectorOfTables(labelOffsets)
	}

	generated.MetadataStart(builder)
	generated.MetadataAddVersion(builder, 1)
	generated.MetadataAddFiles(builder, filesVec)
	if len(labelOffsets) > 0 {
		generated.MetadataAddLabels(builder, labelsVec)
	}
	metadataOff := generated.MetadataEnd(builder)

	builder.Finish(metadataOff)
	return builder.FinishedBytes(), nil
}

// DeserializeMetadata reads FlatBuffers metadata and returns structured data.
func DeserializeMetadata(data []byte) (result *MetadataInfo, err error) {
	defer func() {
		if r := recover(); r != nil {
			result = nil
			err = &ParseError{Message: fmt.Sprintf("invalid flatbuffers data: %v", r)}
		}
	}()

	md := generated.GetRootAsMetadata(data, 0)

	version := md.Version()
	if version != 1 {
		return nil, &MetadataVersionError{Version: version}
	}

	var fileInfos []FileVerityInfo
	if filesLen := md.FilesLength(); filesLen > 0 {
		fileInfos = make([]FileVerityInfo, filesLen)
		var fi generated.FileInfo
		for i := 0; i < filesLen; i++ {
			if !md.Files(&fi, i) {
				return nil, &ParseError{Message: fmt.Sprintf("missing FileInfo at index %d", i)}
			}

			path := string(fi.Path())
			descriptorHash := string(fi.DescriptorHash())

			var fbDesc generated.FsVerityDescriptor
			if fi.Descriptor(&fbDesc) == nil {
				return nil, &ParseError{Message: "missing descriptor for " + path}
			}

			rootHash := fbDesc.RootHashBytes()
			if rootHash == nil {
				return nil, &ParseError{Message: "missing root_hash in descriptor for " + path}
			}

			salt := fbDesc.SaltBytes()
			if salt == nil {
				salt = []byte{}
			}

			descriptor := verity.FsVerityDescriptor{
				Version:       fbDesc.Version(),
				HashAlgorithm: verity.HashAlgorithm(fbDesc.HashAlgorithm()),
				LogBlocksize:  fbDesc.LogBlocksize(),
				DataSize:      fbDesc.DataSize(),
				RootHash:      rootHash,
				Salt:          salt,
			}

			merkleLevel1 := fi.MerkleTreeLevel1Bytes()
			if merkleLevel1 == nil {
				merkleLevel1 = []byte{}
			}

			digestSize := descriptor.HashAlgorithm.DigestSize()
			if len(merkleLevel1)%digestSize != 0 {
				return nil, &ParseError{
					Message: fmt.Sprintf("broken merkle tree for %s: level 1 length %d not a multiple of hash size %d",
						path, len(merkleLevel1), digestSize),
				}
			}

			nHashes := len(merkleLevel1) / digestSize
			hashes := make([][]byte, nHashes)
			for j := 0; j < nHashes; j++ {
				h := make([]byte, digestSize)
				copy(h, merkleLevel1[j*digestSize:(j+1)*digestSize])
				hashes[j] = h
			}

			merkleTree := verity.NewMerkleTree(hashes, descriptor.HashAlgorithm)

			fileInfos[i] = FileVerityInfo{
				Path:           path,
				Descriptor:     descriptor,
				MerkleTree:     merkleTree,
				DescriptorHash: descriptorHash,
			}
		}
	}

	labels := make(map[string]string)
	if labelsLen := md.LabelsLength(); labelsLen > 0 {
		var kv generated.KeyValue
		for i := 0; i < labelsLen; i++ {
			if !md.Labels(&kv, i) {
				return nil, &ParseError{Message: fmt.Sprintf("missing KeyValue at index %d", i)}
			}
			labels[string(kv.Key())] = string(kv.Value())
		}
	}

	return &MetadataInfo{
		FileInfos: fileInfos,
		Labels:    labels,
	}, nil
}

// CalculateFsVerityHash computes the fs-verity descriptor hash for raw file data.
func CalculateFsVerityHash(data []byte) (verity.FsVerityDescriptor, *verity.MerkleTree) {
	d := verity.NewFsVerity(verity.HashSHA256)
	d.Write(data)
	return d.Finalize()
}

func equalBytes(a, b []byte) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
