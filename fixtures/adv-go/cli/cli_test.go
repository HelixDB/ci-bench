package cli

import (
	"os/exec"
	"strings"
	"testing"
)

func TestTool(t *testing.T) {
	out, err := exec.Command("go", "run", Tool).Output()
	if err != nil {
		t.Fatal(err)
	}
	if got, want := strings.TrimSpace(string(out)), "adv-tool 1"; got != want {
		t.Fatalf("go run %s = %q, want %q", Tool, got, want)
	}
}
