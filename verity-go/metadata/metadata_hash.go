// verity-go/metadata/metadata_hash.go
package metadata

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"

	"cryptpilot-verity-go/metadata/generated"

	flatbuffers "github.com/google/flatbuffers/go"
)

// CalculateMetadataHash extracts path+descriptor_hash from metadata, serializes
// to the minimal MetadataHash format, and returns its SHA-256 digest (hex-encoded).
func CalculateMetadataHash(metadataBytes []byte) (string, error) {
	md := generated.GetRootAsMetadata(metadataBytes, 0)

	builder := flatbuffers.NewBuilder(0)

	filesLen := md.FilesLength()
	var filesVec flatbuffers.UOffsetT

	if filesLen > 0 {
		fileOffsets := make([]flatbuffers.UOffsetT, filesLen)
		var fi generated.FileInfo
		for i := 0; i < filesLen; i++ {
			if !md.Files(&fi, i) {
				return "", &ParseError{Message: fmt.Sprintf("missing FileInfo at index %d", i)}
			}
			pathOff := builder.CreateString(string(fi.Path()))
			hashOff := builder.CreateString(string(fi.DescriptorHash()))
			generated.FileHashEntryStart(builder)
			generated.FileHashEntryAddPath(builder, pathOff)
			generated.FileHashEntryAddDescriptorHash(builder, hashOff)
			fileOffsets[i] = generated.FileHashEntryEnd(builder)
		}
		filesVec = builder.CreateVectorOfTables(fileOffsets)
	}

	generated.MetadataHashStart(builder)
	if filesLen > 0 {
		generated.MetadataHashAddFiles(builder, filesVec)
	}
	hashOff := generated.MetadataHashEnd(builder)
	builder.Finish(hashOff)

	h := sha256.New()
	h.Write(builder.FinishedBytes())
	return hex.EncodeToString(h.Sum(nil)), nil
}
