package render

import "testing"

func TestCatalog(t *testing.T) {
	if got, want := Catalog(), "farewell.tmpl,greeting.tmpl"; got != want {
		t.Fatalf("Catalog() = %q, want %q", got, want)
	}
}
