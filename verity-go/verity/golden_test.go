// verity-go/verity/golden_test.go
package verity

import (
	"bytes"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

type goldenFixture struct {
	Name                   string `json:"name"`
	DataB64                string `json:"data_b64"`
	ExpectedDescriptorHash string `json:"expected_descriptor_hash"`
	Generate               string `json:"generate"`
}

func generateTestData(f goldenFixture) ([]byte, error) {
	if f.DataB64 != "" {
		return base64.StdEncoding.DecodeString(f.DataB64)
	}
	switch f.Generate {
	case "empty":
		return []byte{}, nil
	case "b'A'":
		return []byte{'A'}, nil
	case "4096 bytes of 'A'":
		return bytes.Repeat([]byte{'A'}, DefaultBlockSize), nil
	case "4097 bytes: 4096*'A' + 'B'":
		data := bytes.Repeat([]byte{'A'}, DefaultBlockSize+1)
		data[DefaultBlockSize] = 'B'
		return data, nil
	case "4096*128 bytes of 'A'":
		return bytes.Repeat([]byte{'A'}, DefaultBlockSize*128), nil
	case "4096*128-1 bytes of 'A'":
		return bytes.Repeat([]byte{'A'}, DefaultBlockSize*128-1), nil
	case "4096*128+1 bytes of 'A'":
		return bytes.Repeat([]byte{'A'}, DefaultBlockSize*128+1), nil
	default:
		return nil, nil
	}
}

func TestGoldenFixtures(t *testing.T) {
	fixturePath := filepath.Join("golden_fixtures.json")
	data, err := os.ReadFile(fixturePath)
	if err != nil {
		t.Fatalf("failed to read fixtures: %v", err)
	}

	var fixtures []goldenFixture
	if err := json.Unmarshal(data, &fixtures); err != nil {
		t.Fatalf("failed to parse fixtures: %v", err)
	}

	for _, f := range fixtures {
		t.Run(f.Name, func(t *testing.T) {
			testData, err := generateTestData(f)
			if err != nil {
				t.Fatalf("generate test data: %v", err)
			}
			if testData == nil {
				t.Fatalf("unsupported generate field: %q", f.Generate)
			}

			d := NewFsVerity(HashSHA256)
			d.Write(testData)
			desc, tree := d.Finalize()

			actualHash := hex.EncodeToString(desc.ToDescriptorHash())
			if actualHash != f.ExpectedDescriptorHash {
				t.Errorf("descriptor hash mismatch\nexpected: %s\ngot:      %s",
					f.ExpectedDescriptorHash, actualHash)
			}

			rebuiltRoot := tree.RebuildRootHash(desc.Salt, desc.BlockSize())
			if !equalBytes(rebuiltRoot, desc.RootHash) {
				t.Errorf("root hash mismatch after rebuild")
			}
		})
	}
}
