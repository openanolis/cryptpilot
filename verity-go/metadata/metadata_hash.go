// verity-go/metadata/metadata_hash.go
package metadata

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"

	"github.com/gibson042/canonicaljson-go"
	"github.com/openanolis/cryptpilot/verity-go/metadata/generated"
)

// fileHashEntry is a single file entry for hash calculation.
type fileHashEntry struct {
	DescriptorHash string `json:"descriptor_hash"`
	Path           string `json:"path"`
}

// metadataHash is the canonical JSON structure for metadata hash calculation.
type metadataHash struct {
	Files []fileHashEntry `json:"files"`
}

// CalculateMetadataHash extracts path+descriptor_hash from metadata, serializes
// to canonical JSON (sorted by path, sorted keys via struct field order), and
// returns its SHA-256 digest (hex-encoded).
func CalculateMetadataHash(metadataBytes []byte) (string, error) {
	md := generated.GetRootAsMetadata(metadataBytes, 0)

	filesLen := md.FilesLength()
	entries := make([]fileHashEntry, filesLen)

	var fi generated.FileInfo
	for i := 0; i < filesLen; i++ {
		if !md.Files(&fi, i) {
			return "", &ParseError{Message: fmt.Sprintf("missing FileInfo at index %d", i)}
		}
		entries[i] = fileHashEntry{
			DescriptorHash: string(fi.DescriptorHash()),
			Path:           string(fi.Path()),
		}
	}
	// Sort by path for deterministic output.
	sort.Slice(entries, func(i, j int) bool {
		return entries[i].Path < entries[j].Path
	})

	doc := metadataHash{Files: entries}

	jsonBytes, err := canonicaljson.Marshal(doc)
	if err != nil {
		return "", fmt.Errorf("marshal canonical JSON: %w", err)
	}

	h := sha256.New()
	h.Write(jsonBytes)
	return hex.EncodeToString(h.Sum(nil)), nil
}
