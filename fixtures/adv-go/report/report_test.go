package report

import (
	"os"
	"testing"
)

// Reads package catalog's test data at run time (no import of it).
func TestSummaryOfTheCatalog(t *testing.T) {
	data, err := os.ReadFile("../catalog/testdata/items.json")
	if err != nil {
		t.Fatal(err)
	}
	got, err := Summary(data)
	if err != nil {
		t.Fatal(err)
	}
	if want := "apple+pear"; got != want {
		t.Fatalf("Summary = %q, want %q", got, want)
	}
}
